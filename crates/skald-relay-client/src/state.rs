//! Networking-only shared state, owned behind an `Arc` and shared by the WS
//! loop, the pairing/authorization surface, and the QR lookup. Everything here
//! is transport + crypto + the device registry: there is **no** knowledge of
//! what the decrypted bytes mean (the payload-agnostic boundary). Decoded
//! inbound bytes and lifecycle transitions are surfaced via [`RelayEvent`].
//!
//! The wire transport is **v2 protobuf** (docs/relay/relay-protocol.md): every
//! frame queued onto the WS outbound channel is the
//! `prost::Message::encode_to_vec()` of a `RelayFrame`. E2E plaintexts are
//! wrapped in the v2 framing (`compress_payload`) before sealing, and peeled
//! (`decompress_payload`) before being emitted, so consumers only ever see the
//! clean inner payload.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use prost::Message as _;
use skald_relay_common::crypto::{self, DIR_AGENT_TO_CLIENT, DIR_CLIENT_TO_AGENT};
use skald_relay_common::proto::v2::*;
use skald_relay_common::proto::v2::relay_frame::Frame;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use crate::db::{self, ClientRow, ClientState};
use crate::events::RelayEvent;
use crate::identity::Identity;
use crate::pairing::{PairingStore, QrCodeData, SessionState, StartedPairing};

/// Networking config snapshot the runtime needs.
pub(crate) struct StateConfig {
    pub relay_url: String,
    pub pairing_ttl: u32,
}

/// Everything the runloop and surfaces share. Payload-agnostic.
pub(crate) struct RelayState {
    identity: Identity,
    db: Arc<SqlitePool>,
    pairing: PairingStore,
    config: StateConfig,
    /// Sender into the WS outbound queue. `None` until the loop is started.
    /// Carries **encoded protobuf bytes** ready to be wrapped in
    /// `Message::Binary` by the WS layer (v2 transport).
    outbound: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    /// Cache of per-client aes_key, keyed by ed25519 pubkey (crypto.md §8).
    /// Derived from the seed + the client's x25519 pubkey; never persisted.
    aes_cache: Mutex<HashMap<[u8; 32], [u8; 32]>>,
    connected: AtomicBool,
    /// Broadcast sink for [`RelayEvent`]s consumed by the application layer.
    events_tx: broadcast::Sender<RelayEvent>,
}

impl RelayState {
    pub(crate) fn new(
        identity: Identity,
        db: Arc<SqlitePool>,
        config: StateConfig,
        events_tx: broadcast::Sender<RelayEvent>,
    ) -> Self {
        Self {
            identity,
            db,
            pairing: PairingStore::new(),
            config,
            outbound: Mutex::new(None),
            aes_cache: Mutex::new(HashMap::new()),
            connected: AtomicBool::new(false),
            events_tx,
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub(crate) fn identity(&self) -> &Identity {
        &self.identity
    }

    pub(crate) fn relay_url(&self) -> String {
        self.config.relay_url.clone()
    }

    pub(crate) fn default_pairing_ttl(&self) -> u32 {
        self.config.pairing_ttl
    }

    /// Emit a [`RelayEvent`]; ignores the "no subscribers" case.
    pub(crate) fn emit(&self, ev: RelayEvent) {
        let _ = self.events_tx.send(ev);
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<RelayEvent> {
        self.events_tx.subscribe()
    }

    pub(crate) fn set_connected(&self, v: bool) {
        let was = self.connected.swap(v, Ordering::Relaxed);
        if was != v {
            self.emit(if v { RelayEvent::Connected } else { RelayEvent::Disconnected });
        }
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub(crate) fn set_outbound(&self, tx: mpsc::UnboundedSender<Vec<u8>>) {
        *self.outbound.lock().unwrap() = Some(tx);
    }

    pub(crate) fn clear_outbound(&self) {
        *self.outbound.lock().unwrap() = None;
    }

    /// Queue an already-encoded `RelayFrame` onto the WS outbound channel.
    fn send_frame(&self, bytes: Vec<u8>) -> Result<()> {
        let guard = self.outbound.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx
                .send(bytes)
                .map_err(|_| anyhow::anyhow!("WS outbound channel closed")),
            None => Err(anyhow::anyhow!("WS not started")),
        }
    }

    pub(crate) async fn authorized_pubkeys_hex(&self) -> Result<Vec<String>> {
        db::authorized_pubkeys_hex(&self.db).await
    }

    /// Re-send the full authorize set (replacement semantics,
    /// relay-protocol.md §7). v2: each client pubkey travels as a raw 32-byte
    /// `bytes` field.
    async fn send_authorize(&self) -> Result<()> {
        let clients_hex = db::authorized_pubkeys_hex(&self.db).await?;
        let clients: Vec<prost::bytes::Bytes> = clients_hex
            .iter()
            .filter_map(|h| hex::decode(h).ok())
            .map(prost::bytes::Bytes::from)
            .collect();
        let frame = RelayFrame {
            frame: Some(Frame::Authorize(Authorize { clients })),
        };
        self.send_frame(frame.encode_to_vec())
    }

    // ── Pairing ───────────────────────────────────────────────────────────────

    /// Open a pairing window: generate a token, send `pairing_start`, register
    /// the in-memory session (latest-wins). Returns the handle for the QR URL.
    pub(crate) async fn start_pairing(&self, ttl_secs: u32) -> Result<StartedPairing> {
        let started = self.pairing.start(
            &self.config.relay_url,
            self.identity.namespace_id_hex(),
            &self.identity.ed25519_pub(),
            &self.identity.x25519_pub(),
            ttl_secs,
        );
        let frame = RelayFrame {
            frame: Some(Frame::PairingStart(PairingStart {
                pairing_token: prost::bytes::Bytes::copy_from_slice(&started.token),
                ttl: ttl_secs,
            })),
        };
        self.send_frame(frame.encode_to_vec())?;
        debug!(crate_name = "skald-relay-client", ttl_secs, "pairing window opened");
        Ok(started)
    }

    /// Close the pairing window locally and tell the relay.
    pub(crate) async fn stop_pairing(&self) -> Result<()> {
        self.pairing.supersede_all();
        let frame = RelayFrame {
            frame: Some(Frame::PairingStop(PairingStop {})),
        };
        self.send_frame(frame.encode_to_vec())
    }

    /// Look up a pairing session for the QR endpoint.
    pub(crate) fn lookup_pairing(&self, code: &str) -> Option<(QrCodeData, SessionState)> {
        self.pairing.lookup(code)
    }

    /// Handle `client_paired` (relay-protocol.md §6 step 7): derive aes_key,
    /// persist the client as Pending, consume the pairing session, then emit
    /// [`RelayEvent::ClientPaired`]. The **authorization policy is the
    /// consumer's** — this layer never auto-authorizes.
    pub(crate) async fn handle_client_paired(
        &self,
        client_ed25519_pub: &[u8; 32],
        client_x25519_pub: &[u8; 32],
        platform: &str,
    ) {
        let ed = *client_ed25519_pub;
        let x = *client_x25519_pub;

        // Derive + cache the per-client aes_key.
        let aes_key = self.identity.derive_aes_key(&x);
        self.aes_cache.lock().unwrap().insert(ed, aes_key);

        // Persist as Pending with counters at 0.
        if let Err(e) = db::upsert_paired(&self.db, &ed, &x, Some(platform)).await {
            warn!(crate_name = "skald-relay-client", error = %e, "failed to persist paired client");
            return;
        }

        // Mark the active pairing session as consumed.
        if let Some(tok) = self.pairing.active_token() {
            self.pairing.consume_by_token(&tok);
        }

        self.emit(RelayEvent::ClientPaired {
            ed25519_pub: ed,
            x25519_pub: x,
            platform: platform.to_string(),
        });
    }

    /// Mark a client Authorized and push the updated authorize set. Does NOT
    /// broadcast any application payload — that is the consumer's job after
    /// authorizing (the client is payload-agnostic).
    pub(crate) async fn authorize(&self, ed25519_pub: &[u8; 32]) -> Result<()> {
        db::set_authorized(&self.db, ed25519_pub).await?;
        self.send_authorize().await?;
        debug!(crate_name = "skald-relay-client", device = %hex::encode(ed25519_pub), "device authorized");
        Ok(())
    }

    /// Revoke a client (relay-protocol.md §7): drop from the set, re-authorize
    /// without it, delete its keys/counters/device_info, emit `ClientRevoked`.
    pub(crate) async fn revoke(&self, ed25519_pub: &[u8; 32]) -> Result<()> {
        db::delete(&self.db, ed25519_pub).await?;
        self.aes_cache.lock().unwrap().remove(ed25519_pub);
        self.send_authorize().await?;
        debug!(crate_name = "skald-relay-client", device = %hex::encode(ed25519_pub), "device revoked");
        self.emit(RelayEvent::ClientRevoked { ed25519_pub: *ed25519_pub });
        Ok(())
    }

    /// Remove every device, clear the aes cache, and push an empty authorize
    /// set. Emits one `ClientRevoked` per removed device.
    pub(crate) async fn clear_all(&self) -> Result<()> {
        let removed = db::list_all(&self.db).await.unwrap_or_default();
        db::delete_all(&self.db).await?;
        self.aes_cache.lock().unwrap().clear();
        self.send_authorize().await?;
        for c in removed {
            self.emit(RelayEvent::ClientRevoked { ed25519_pub: c.ed25519_pub });
        }
        Ok(())
    }

    /// Persist the device_info JSON for a client (from a `hello` payload, decoded
    /// by the consumer).
    pub(crate) async fn set_device_info(&self, ed25519_pub: &[u8; 32], json: &str) -> Result<()> {
        db::set_device_info(&self.db, ed25519_pub, json).await
    }

    pub(crate) async fn list_clients(&self) -> Vec<ClientRow> {
        db::list_all(&self.db).await.unwrap_or_default()
    }

    // ── E2E: aes_key cache ────────────────────────────────────────────────────

    /// Resolve (and cache) the aes_key for a client, deriving from the stored
    /// x25519 pubkey on a cache miss.
    async fn aes_key_for(&self, ed25519_pub: &[u8; 32]) -> Option<[u8; 32]> {
        if let Some(k) = self.aes_cache.lock().unwrap().get(ed25519_pub) {
            return Some(*k);
        }
        let row = db::get(&self.db, ed25519_pub).await.ok().flatten()?;
        let key = self.identity.derive_aes_key(&row.x25519_pub);
        self.aes_cache.lock().unwrap().insert(*ed25519_pub, key);
        Some(key)
    }

    // ── Send ──────────────────────────────────────────────────────────────────

    /// Seal an opaque `payload` to one client and queue the `message` frame.
    ///
    /// v2 transport: the payload is wrapped in the `version ‖ comp ‖ payload`
    /// framing (`compress_payload`) before sealing, then wrapped in
    /// `RelayFrame{Message{ciphertext, nonce, peer, live}}`. `live=true` routes
    /// or fails (the peer is online by construction); `live=false` stores-and-
    /// forwards + pushes for offline phones.
    pub(crate) async fn send_to_client(
        &self,
        client_ed25519_pub: &[u8; 32],
        payload: &[u8],
        live: bool,
    ) -> Result<()> {
        let aes_key = self
            .aes_key_for(client_ed25519_pub)
            .await
            .ok_or_else(|| anyhow::anyhow!("no aes_key for client"))?;

        // Persist the send counter BEFORE sealing/sending (crypto.md §8/§9):
        // a crash after this point never reuses a nonce.
        let counter = db::next_send_counter(&self.db, client_ed25519_pub).await?;
        let nonce = crypto::build_nonce(DIR_AGENT_TO_CLIENT, counter);
        let aad = crypto::build_aad(
            &self.identity.namespace_id_raw(),
            &self.identity.ed25519_pub(),
            client_ed25519_pub,
        );
        // v2 framing: version(1B) ‖ comp(1B) ‖ payload (compresses over threshold).
        let framed = crypto::compress_payload(payload);
        let sealed = crypto::seal(&aes_key, &nonce, &aad, &framed)
            .map_err(|e| anyhow::anyhow!("seal failed: {e}"))?;

        let frame = RelayFrame {
            frame: Some(Frame::Message(Message {
                ciphertext: prost::bytes::Bytes::from(sealed),
                nonce: prost::bytes::Bytes::copy_from_slice(&nonce),
                peer: prost::bytes::Bytes::copy_from_slice(client_ed25519_pub),
                live,
            })),
        };
        self.send_frame(frame.encode_to_vec())
    }

    // ── Receive ───────────────────────────────────────────────────────────────

    /// Handle an inbound `message` (relay-protocol.md §3.1): authorize the
    /// sender, check nonce direction + counter monotonicity, open, advance the
    /// recv counter, peel the v2 framing, then emit [`RelayEvent::Message`] with
    /// the clean inner payload. The client never inspects the payload contents.
    pub(crate) async fn handle_inbound_message(
        &self,
        from: &[u8; 32],
        nonce: &[u8; 12],
        ciphertext: &[u8],
        live: bool,
    ) {
        // `from` must be an Authorized client.
        let row = match db::get(&self.db, from).await {
            Ok(Some(r)) if r.state == ClientState::Authorized => r,
            _ => {
                warn!(crate_name = "skald-relay-client", "message from non-authorized sender dropped");
                return;
            }
        };

        // Extract the counter from the nonce and check direction + monotonicity.
        if nonce[..4] != DIR_CLIENT_TO_AGENT {
            warn!(crate_name = "skald-relay-client", "message with wrong nonce direction dropped");
            return;
        }
        let counter = u64::from_be_bytes(nonce[4..].try_into().unwrap());
        if counter <= row.recv_counter {
            warn!(crate_name = "skald-relay-client", "replayed/old counter dropped");
            return;
        }

        let Some(aes_key) = self.aes_key_for(from).await else { return };
        let aad = crypto::build_aad(
            &self.identity.namespace_id_raw(),
            from,
            &self.identity.ed25519_pub(),
        );
        let framed = match crypto::open(&aes_key, nonce, &aad, ciphertext) {
            Ok(pt) => pt,
            Err(_) => {
                // No content logging on decrypt failure (crypto.md §8).
                warn!(crate_name = "skald-relay-client", "decrypt failed, message dropped");
                return;
            }
        };

        // Valid open → advance recv_counter.
        if let Err(e) = db::set_recv_counter(&self.db, from, counter).await {
            warn!(crate_name = "skald-relay-client", error = %e, "failed to persist recv_counter");
        }

        // Peel the v2 framing so the consumer sees the clean inner payload.
        let payload = match crypto::decompress_payload(&framed) {
            Ok(p) => p,
            Err(e) => {
                warn!(crate_name = "skald-relay-client", error = %e, "framing decompress failed");
                return;
            }
        };

        self.emit(RelayEvent::Message { from: *from, payload, live });
    }
}

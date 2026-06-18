//! Shared runtime state for the mobile-connector, owned behind an `Arc` and
//! shared by the WS loop, the bus subscriber, the QR router, and the control
//! tools. All cross-flow logic (pairing policy, E2E seal/open, Inbox
//! application) lives here so each surface stays thin.
//!
//! The wire transport is **v2 protobuf** (data/iOS-app/v2/relay-protocol.md):
//! every frame queued onto the WS outbound channel is the
//! `prost::Message::encode_to_vec()` of a `RelayFrame`. E2E plaintexts are
//! wrapped in the v2 framing (`compress_payload`) before sealing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use prost::Message as _;
use skald_relay_common::crypto::{self, DIR_AGENT_TO_CLIENT, DIR_CLIENT_TO_AGENT};
use skald_relay_common::proto::v2::*;
use skald_relay_common::proto::v2::relay_frame::Frame;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use core_api::inbox::InboxApi;

use crate::db::{self, ClientState};
use crate::identity::Identity;
use crate::pairing::{PairingStore, QrCodeData, SessionState, StartedPairing};
use crate::payloads::{self, ClientPayload};

/// Configuration snapshot the runtime needs (subset of the plugin config).
pub struct RelayConfig {
    pub relay_url: String,
    pub pairing_ttl: u32,
    pub require_device_confirmation: bool,
}

/// Everything the runloop and surfaces share.
pub struct RelayState {
    identity: Identity,
    db: Arc<SqlitePool>,
    inbox: Arc<dyn InboxApi>,
    pairing: PairingStore,
    config: RelayConfig,
    /// Sender into the WS outbound queue. `None` until the loop is started.
    /// Carries **encoded protobuf bytes** ready to be wrapped in
    /// `Message::Binary` by the WS layer (v2 transport).
    outbound: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    /// Cache of per-client aes_key, keyed by ed25519 pubkey (plugin.md §8).
    /// Derived from the seed + the client's x25519 pubkey; never persisted.
    aes_cache: Mutex<HashMap<[u8; 32], [u8; 32]>>,
    connected: AtomicBool,
}

impl RelayState {
    pub fn new(
        identity: Identity,
        db: Arc<SqlitePool>,
        inbox: Arc<dyn InboxApi>,
        config: RelayConfig,
    ) -> Self {
        Self {
            identity,
            db,
            inbox,
            pairing: PairingStore::new(),
            config,
            outbound: Mutex::new(None),
            aes_cache: Mutex::new(HashMap::new()),
            connected: AtomicBool::new(false),
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn relay_url(&self) -> String {
        self.config.relay_url.clone()
    }

    pub fn set_connected(&self, v: bool) {
        self.connected.store(v, Ordering::Relaxed);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn set_outbound(&self, tx: mpsc::UnboundedSender<Vec<u8>>) {
        *self.outbound.lock().unwrap() = Some(tx);
    }

    pub fn clear_outbound(&self) {
        *self.outbound.lock().unwrap() = None;
    }

    /// Queue an already-encoded `RelayFrame` onto the WS outbound channel.
    /// The caller builds the protobuf struct, calls `encode_to_vec()`, and
    /// hands the resulting bytes off here.
    fn send_frame(&self, bytes: Vec<u8>) -> Result<()> {
        let guard = self.outbound.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx
                .send(bytes)
                .map_err(|_| anyhow::anyhow!("WS outbound channel closed")),
            None => Err(anyhow::anyhow!("WS not started")),
        }
    }

    pub async fn authorized_pubkeys_hex(&self) -> Result<Vec<String>> {
        db::authorized_pubkeys_hex(&self.db).await
    }

    /// Re-send the full authorize set (replacement semantics, plugin.md §7).
    /// v2: each client pubkey travels as a raw 32-byte `bytes` field — no more
    /// hex strings on the wire.
    async fn send_authorize(&self) -> Result<()> {
        let clients_hex = db::authorized_pubkeys_hex(&self.db).await?;
        let clients: Vec<prost::bytes::Bytes> = clients_hex
            .iter()
            .filter_map(|h| hex::decode(h).ok())
            .map(|b| prost::bytes::Bytes::from(b))
            .collect();
        let frame = RelayFrame {
            frame: Some(Frame::Authorize(Authorize { clients })),
        };
        self.send_frame(frame.encode_to_vec())
    }

    // ── Pairing ───────────────────────────────────────────────────────────────

    /// Open a pairing window: generate a token, send `pairing_start`, register
    /// the in-memory session (latest-wins). Returns the handle for the QR URL.
    pub async fn start_pairing(&self, ttl_secs: u32) -> Result<StartedPairing> {
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
        info!(plugin = "mobile-connector", ttl_secs, "pairing window opened");
        Ok(started)
    }

    /// Close the pairing window locally and tell the relay.
    pub async fn stop_pairing(&self) -> Result<()> {
        self.pairing.supersede_all();
        let frame = RelayFrame {
            frame: Some(Frame::PairingStop(PairingStop {})),
        };
        self.send_frame(frame.encode_to_vec())
    }

    /// Look up a pairing session for the QR endpoint.
    pub fn lookup_pairing(&self, code: &str) -> Option<(QrCodeData, SessionState)> {
        self.pairing.lookup(code)
    }

    pub fn default_pairing_ttl(&self) -> u32 {
        self.config.pairing_ttl
    }

    /// Handle `client_paired` (plugin.md §6 step 7): derive aes_key, persist the
    /// client as Pending, consume the pairing session, then apply the policy.
    ///
    /// Inputs are raw 32-byte arrays (v2 transport): the WS layer copies them
    /// out of the `ClientPaired` protobuf message before calling. `platform` is
    /// the lowercase string decoded from the protobuf `Platform` enum.
    pub async fn handle_client_paired(
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
            warn!(plugin = "mobile-connector", error = %e, "failed to persist paired client");
            return;
        }

        // Mark the active pairing session as consumed.
        if let Some(tok) = self.pairing.active_token() {
            self.pairing.consume_by_token(&tok);
        }

        // Authorization policy (plugin.md §6 step 7e).
        if self.config.require_device_confirmation {
            // Manual: the human confirms via Skald's Inbox. We surface a generic
            // notification toward existing authorized clients and rely on the
            // copilot/Inbox UI to confirm; once confirmed, the operator calls
            // the authorize path. For an unattended-friendly default we record
            // the device Pending and log — the operator authorizes via the tool
            // surface (mobile_list_devices shows Pending devices).
            info!(
                plugin = "mobile-connector",
                device = %hex::encode(ed),
                "new device paired (pending manual confirmation)"
            );
            let _ = self
                .broadcast_notification("Nuovo device", "Un nuovo device è in attesa di conferma")
                .await;
        } else {
            // Auto-authorize.
            if let Err(e) = self.authorize_client(&ed).await {
                warn!(plugin = "mobile-connector", error = %e, "auto-authorize failed");
            }
        }
    }

    /// Mark a client Authorized and push the updated authorize set.
    pub async fn authorize_client(&self, ed25519_pub: &[u8; 32]) -> Result<()> {
        db::set_authorized(&self.db, ed25519_pub).await?;
        self.send_authorize().await?;
        info!(plugin = "mobile-connector", device = %hex::encode(ed25519_pub), "device authorized");
        // Send the current Inbox snapshot to the newly-authorized device.
        let _ = self.broadcast_inbox().await;
        Ok(())
    }

    /// Revoke a client (plugin.md §7): drop from the set, re-authorize without
    /// it, delete its keys/counters/device_info.
    pub async fn revoke_client(&self, ed25519_pub: &[u8; 32]) -> Result<()> {
        db::delete(&self.db, ed25519_pub).await?;
        self.aes_cache.lock().unwrap().remove(ed25519_pub);
        self.send_authorize().await?;
        info!(plugin = "mobile-connector", device = %hex::encode(ed25519_pub), "device revoked");
        Ok(())
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

    // ── Inbox → clients ───────────────────────────────────────────────────────

    /// Build the Inbox snapshot and send it (encrypted) to every Authorized
    /// client (plugin.md §8 "Inbox → client").
    pub async fn broadcast_inbox(&self) -> Result<()> {
        let snapshot = self.inbox.list_pending().await;
        let payload = payloads::build_inbox_update(&snapshot);
        let plaintext = serde_json::to_vec(&payload)?;
        self.broadcast_plaintext(&plaintext).await
    }

    /// Send the current Inbox snapshot to a single client (payloads.md §4.6).
    /// This is the targeted reply to `inbox_request`: unlike [`broadcast_inbox`],
    /// it seals only to `client_ed25519_pub` and never re-aligns other devices.
    /// Marked `live=true` because the requester is online by construction
    /// (relay-protocol.md §3.1) — the relay can deliver it directly.
    async fn send_inbox_to(&self, client_ed25519_pub: &[u8; 32]) -> Result<()> {
        let snapshot = self.inbox.list_pending().await;
        let payload = payloads::build_inbox_update(&snapshot);
        let plaintext = serde_json::to_vec(&payload)?;
        self.send_to_client(client_ed25519_pub, &plaintext, true).await
    }

    /// Build and send a generic notification to all Authorized clients.
    pub async fn broadcast_notification(&self, title: &str, body: &str) -> Result<()> {
        let payload = payloads::build_notification(title, body);
        let plaintext = serde_json::to_vec(&payload)?;
        self.broadcast_plaintext(&plaintext).await
    }

    /// Seal `plaintext` per-client and queue a `message` frame for each
    /// Authorized device. Marked `live=false` so the relay stores-and-forwards
    /// (and pushes via APNs/FCM) for phones that are currently offline.
    async fn broadcast_plaintext(&self, plaintext: &[u8]) -> Result<()> {
        let clients = db::list_all(&self.db).await?;
        for c in clients.into_iter().filter(|c| c.state == ClientState::Authorized) {
            if let Err(e) = self
                .send_to_client(&c.ed25519_pub, plaintext, false)
                .await
            {
                warn!(plugin = "mobile-connector", error = %e, "failed to send to client");
            }
        }
        Ok(())
    }

    /// Seal a plaintext to one client and queue the `message` frame.
    ///
    /// v2 transport: the plaintext is wrapped in the `version ‖ comp ‖ payload`
    /// framing (`compress_payload`) before sealing, and the resulting envelope
    /// is wrapped in `RelayFrame{Message{ciphertext, nonce, peer, live}}`.
    /// `live` is the caller's choice:
    /// - `true` for the targeted reply to an `inbox_request` (the client is
    ///   online by construction, relay §3.1).
    /// - `false` for unsolicited broadcasts (new approvals / clarifications)
    ///   that must reach an offline phone via store-and-forward + push.
    async fn send_to_client(
        &self,
        client_ed25519_pub: &[u8; 32],
        plaintext: &[u8],
        live: bool,
    ) -> Result<()> {
        let aes_key = self
            .aes_key_for(client_ed25519_pub)
            .await
            .ok_or_else(|| anyhow::anyhow!("no aes_key for client"))?;

        // Persist the send counter BEFORE sealing/sending (plugin.md §8/§9):
        // a crash after this point never reuses a nonce.
        let counter = db::next_send_counter(&self.db, client_ed25519_pub).await?;
        let nonce = crypto::build_nonce(DIR_AGENT_TO_CLIENT, counter);
        let aad = crypto::build_aad(
            &self.identity.namespace_id_raw(),
            &self.identity.ed25519_pub(),
            client_ed25519_pub,
        );
        // v2 framing: version(1B) ‖ comp(1B) ‖ payload. Compresses only if the
        // payload is larger than `COMPRESS_THRESHOLD` (framing.md §2.3).
        let framed_plaintext = crypto::compress_payload(plaintext);
        let sealed = crypto::seal(&aes_key, &nonce, &aad, &framed_plaintext)
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

    // ── Clients → Inbox ───────────────────────────────────────────────────────

    /// Handle an inbound `message` (plugin.md §8 "client → Inbox").
    ///
    /// v2 transport: inputs are already raw bytes — the WS layer copies them
    /// out of the `Message` protobuf (ciphertext/nonce/peer) before calling.
    /// `ciphertext` is the AES-GCM-sealed payload, which when opened contains
    /// the v2-framed plaintext (`version ‖ comp ‖ json`); `decompress_payload`
    /// peels off the framing before `parse_client_payload` reads the JSON.
    pub async fn handle_inbound_message(
        &self,
        from: &[u8; 32],
        nonce: &[u8; 12],
        ciphertext: &[u8],
    ) {
        // from must be an Authorized client.
        let row = match db::get(&self.db, from).await {
            Ok(Some(r)) if r.state == ClientState::Authorized => r,
            _ => {
                warn!(plugin = "mobile-connector", "message from non-authorized sender dropped");
                return;
            }
        };

        // Extract the counter from the nonce and check direction + monotonicity.
        if nonce[..4] != DIR_CLIENT_TO_AGENT {
            warn!(plugin = "mobile-connector", "message with wrong nonce direction dropped");
            return;
        }
        let counter = u64::from_be_bytes(nonce[4..].try_into().unwrap());
        if counter <= row.recv_counter {
            warn!(plugin = "mobile-connector", "replayed/old counter dropped");
            return;
        }

        let Some(aes_key) = self.aes_key_for(from).await else { return };
        let aad = crypto::build_aad(
            &self.identity.namespace_id_raw(),
            from,
            &self.identity.ed25519_pub(),
        );
        let framed_plaintext = match crypto::open(&aes_key, nonce, &aad, ciphertext) {
            Ok(pt) => pt,
            Err(_) => {
                // No content logging on decrypt failure (plugin.md §8).
                warn!(plugin = "mobile-connector", "decrypt failed, message dropped");
                return;
            }
        };

        // Valid open → advance recv_counter.
        if let Err(e) = db::set_recv_counter(&self.db, from, counter).await {
            warn!(plugin = "mobile-connector", error = %e, "failed to persist recv_counter");
        }

        self.apply_client_payload(from, &framed_plaintext).await;
    }

    /// Apply a decoded client payload to the Inbox, then re-snapshot. The
    /// `plaintext` argument is the *v2-framed* body straight out of AES-GCM
    /// (`version ‖ comp ‖ payload`); this fn peels the framing with
    /// `decompress_payload` before `parse_client_payload` reads the JSON.
    async fn apply_client_payload(&self, from: &[u8; 32], plaintext: &[u8]) {
        let payload = match crypto::decompress_payload(plaintext) {
            Ok(p) => p,
            Err(e) => {
                warn!(plugin = "mobile-connector", error = %e, "framing decompress failed");
                return;
            }
        };
        match payloads::parse_client_payload(&payload) {
            ClientPayload::ApprovalResponse { request_id, approved, reason } => {
                if approved {
                    self.inbox.approve(request_id).await;
                } else {
                    self.inbox.reject(request_id, reason.unwrap_or_default()).await;
                }
                let _ = self.broadcast_inbox().await;
            }
            ClientPayload::ClarificationResponse { request_id, answer } => {
                self.inbox.answer(request_id, answer).await;
                let _ = self.broadcast_inbox().await;
            }
            ClientPayload::Hello { device_info } => {
                if let Err(e) =
                    db::set_device_info(&self.db, from, &device_info.to_string()).await
                {
                    warn!(plugin = "mobile-connector", error = %e, "failed to persist device_info");
                }
            }
            ClientPayload::InboxRequest => {
                // Targeted reply: snapshot only the requester, no bus side-effects.
                if let Err(e) = self.send_inbox_to(from).await {
                    warn!(plugin = "mobile-connector", error = %e, "failed to send targeted inbox snapshot");
                }
            }
            ClientPayload::Logout => {
                if let Err(e) = self.revoke_client(from).await {
                    warn!(plugin = "mobile-connector", error = %e, "logout revoke failed");
                }
            }
            ClientPayload::Unknown => {
                debug!(plugin = "mobile-connector", "unknown/ignored client payload");
            }
        }
    }

    // ── Device listing ────────────────────────────────────────────────────────

    pub async fn list_clients(&self) -> Vec<db::ClientRow> {
        db::list_all(&self.db).await.unwrap_or_default()
    }
}

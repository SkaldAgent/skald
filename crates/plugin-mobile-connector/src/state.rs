//! Shared runtime state for the mobile-connector, owned behind an `Arc` and
//! shared by the WS loop, the bus subscriber, the QR router, and the control
//! tools. All cross-flow logic (pairing policy, E2E seal/open, Inbox
//! application) lives here so each surface stays thin.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use base64::Engine;
use skald_relay_common::crypto::{self, DIR_AGENT_TO_CLIENT, DIR_CLIENT_TO_AGENT};
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
    outbound: Mutex<Option<mpsc::UnboundedSender<String>>>,
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

    pub fn set_outbound(&self, tx: mpsc::UnboundedSender<String>) {
        *self.outbound.lock().unwrap() = Some(tx);
    }

    pub fn clear_outbound(&self) {
        *self.outbound.lock().unwrap() = None;
    }

    /// Queue a serialized frame onto the WS outbound channel.
    fn send_frame(&self, text: String) -> Result<()> {
        let guard = self.outbound.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx
                .send(text)
                .map_err(|_| anyhow::anyhow!("WS outbound channel closed")),
            None => Err(anyhow::anyhow!("WS not started")),
        }
    }

    pub async fn authorized_pubkeys_hex(&self) -> Result<Vec<String>> {
        db::authorized_pubkeys_hex(&self.db).await
    }

    /// Re-send the full authorize set (replacement semantics, plugin.md §7).
    async fn send_authorize(&self) -> Result<()> {
        let clients = db::authorized_pubkeys_hex(&self.db).await?;
        let frame = serde_json::json!({ "type": "authorize", "clients": clients });
        self.send_frame(frame.to_string())
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
        let frame = serde_json::json!({
            "type": "pairing_start",
            "pairing_token": hex::encode(started.token),
            "ttl": ttl_secs,
        });
        self.send_frame(frame.to_string())?;
        info!(plugin = "mobile-connector", ttl_secs, "pairing window opened");
        Ok(started)
    }

    /// Close the pairing window locally and tell the relay.
    pub async fn stop_pairing(&self) -> Result<()> {
        self.pairing.supersede_all();
        self.send_frame(r#"{"type":"pairing_stop"}"#.to_string())
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
    pub async fn handle_client_paired(
        &self,
        client_ed25519_pub_hex: &str,
        client_x25519_pub_hex: &str,
        platform: &str,
    ) {
        let (Some(ed), Some(x)) = (
            crypto::decode_hex::<32>(client_ed25519_pub_hex),
            crypto::decode_hex::<32>(client_x25519_pub_hex),
        ) else {
            warn!(plugin = "mobile-connector", "client_paired with malformed pubkeys, ignoring");
            return;
        };

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
                device = client_ed25519_pub_hex,
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
    async fn send_inbox_to(&self, client_ed25519_pub: &[u8; 32]) -> Result<()> {
        let snapshot = self.inbox.list_pending().await;
        let payload = payloads::build_inbox_update(&snapshot);
        let plaintext = serde_json::to_vec(&payload)?;
        self.send_to_client(client_ed25519_pub, &plaintext).await
    }

    /// Build and send a generic notification to all Authorized clients.
    pub async fn broadcast_notification(&self, title: &str, body: &str) -> Result<()> {
        let payload = payloads::build_notification(title, body);
        let plaintext = serde_json::to_vec(&payload)?;
        self.broadcast_plaintext(&plaintext).await
    }

    /// Seal `plaintext` per-client and queue a `message` frame for each
    /// Authorized device.
    async fn broadcast_plaintext(&self, plaintext: &[u8]) -> Result<()> {
        let clients = db::list_all(&self.db).await?;
        for c in clients.into_iter().filter(|c| c.state == ClientState::Authorized) {
            if let Err(e) = self.send_to_client(&c.ed25519_pub, plaintext).await {
                warn!(plugin = "mobile-connector", error = %e, "failed to send to client");
            }
        }
        Ok(())
    }

    /// Seal a plaintext to one client and queue the `message` frame.
    async fn send_to_client(&self, client_ed25519_pub: &[u8; 32], plaintext: &[u8]) -> Result<()> {
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
        let sealed = crypto::seal(&aes_key, &nonce, &aad, plaintext)
            .map_err(|e| anyhow::anyhow!("seal failed: {e}"))?;

        let frame = serde_json::json!({
            "type": "message",
            "to": hex::encode(client_ed25519_pub),
            "nonce": hex::encode(nonce),
            "ciphertext": base64::engine::general_purpose::STANDARD.encode(sealed),
        });
        self.send_frame(frame.to_string())
    }

    // ── Clients → Inbox ───────────────────────────────────────────────────────

    /// Handle an inbound `message` (plugin.md §8 "client → Inbox").
    pub async fn handle_inbound_message(&self, from_hex: &str, nonce_hex: &str, ciphertext_b64: &str) {
        let Some(from) = crypto::decode_hex::<32>(from_hex) else { return };

        // from must be an Authorized client.
        let row = match db::get(&self.db, &from).await {
            Ok(Some(r)) if r.state == ClientState::Authorized => r,
            _ => {
                warn!(plugin = "mobile-connector", "message from non-authorized sender dropped");
                return;
            }
        };

        let Some(nonce_bytes) = crypto::decode_hex::<12>(nonce_hex) else { return };
        // Extract the counter from the nonce and check direction + monotonicity.
        if nonce_bytes[..4] != DIR_CLIENT_TO_AGENT {
            warn!(plugin = "mobile-connector", "message with wrong nonce direction dropped");
            return;
        }
        let counter = u64::from_be_bytes(nonce_bytes[4..].try_into().unwrap());
        if counter <= row.recv_counter {
            warn!(plugin = "mobile-connector", "replayed/old counter dropped");
            return;
        }

        let Ok(ciphertext) = base64::engine::general_purpose::STANDARD.decode(ciphertext_b64) else {
            return;
        };
        let Some(aes_key) = self.aes_key_for(&from).await else { return };
        let aad = crypto::build_aad(
            &self.identity.namespace_id_raw(),
            &from,
            &self.identity.ed25519_pub(),
        );
        let plaintext = match crypto::open(&aes_key, &nonce_bytes, &aad, &ciphertext) {
            Ok(pt) => pt,
            Err(_) => {
                // No content logging on decrypt failure (plugin.md §8).
                warn!(plugin = "mobile-connector", "decrypt failed, message dropped");
                return;
            }
        };

        // Valid open → advance recv_counter.
        if let Err(e) = db::set_recv_counter(&self.db, &from, counter).await {
            warn!(plugin = "mobile-connector", error = %e, "failed to persist recv_counter");
        }

        self.apply_client_payload(&from, &plaintext).await;
    }

    /// Apply a decoded client payload to the Inbox, then re-snapshot.
    async fn apply_client_payload(&self, from: &[u8; 32], plaintext: &[u8]) {
        match payloads::parse_client_payload(plaintext) {
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

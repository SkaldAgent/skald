//! The permanent agent WebSocket toward the relay (plugin.md §3,
//! relay-protocol.md §4.1/§8).
//!
//! A single WS carries everything: challenge-response auth, the `authorize` set,
//! outbound E2E `message`s, and inbound `message` / `client_paired` frames.
//! Reconnection uses exponential backoff (1,2,4,…,60 s) with jitter, and the
//! whole loop is cancellable on stop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use skald_relay_common::crypto;
use skald_relay_common::frames::{Incoming, Outgoing};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::state::RelayState;

/// Run the reconnecting WS loop until `cancel` fires (plugin.md §3 step 6).
pub async fn run_loop(
    state: Arc<RelayState>,
    mut outbound_rx: mpsc::UnboundedReceiver<String>,
    cancel: CancellationToken,
) {
    let mut backoff_step: u32 = 0;
    loop {
        if cancel.is_cancelled() {
            return;
        }

        match connect_once(&state, &mut outbound_rx, &cancel).await {
            Ok(()) => {
                // Clean disconnect (cancelled or graceful): reset backoff.
                backoff_step = 0;
            }
            Err(e) => {
                warn!(plugin = "mobile-connector", error = %e, "relay connection ended");
            }
        }

        if cancel.is_cancelled() {
            return;
        }

        let delay = backoff_delay(backoff_step);
        backoff_step = backoff_step.saturating_add(1);
        state.set_connected(false);
        debug!(plugin = "mobile-connector", secs = delay.as_secs_f64(), "reconnect backoff");
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

/// Backoff schedule 1,2,4,…,60 s plus up to 50% jitter (relay-protocol.md §8).
fn backoff_delay(step: u32) -> Duration {
    let base = 1u64.checked_shl(step).unwrap_or(60).min(60);
    let jitter_ms = rand::rng().random_range(0..=(base * 500));
    Duration::from_millis(base * 1000 + jitter_ms)
}

/// One full connection lifecycle: connect → challenge → auth → authorize → loop.
async fn connect_once(
    state: &Arc<RelayState>,
    outbound_rx: &mut mpsc::UnboundedReceiver<String>,
    cancel: &CancellationToken,
) -> Result<()> {
    let url = state.relay_url();
    info!(plugin = "mobile-connector", %url, "connecting to relay");

    let (ws_stream, _resp) = tokio::select! {
        _ = cancel.cancelled() => return Ok(()),
        r = tokio_tungstenite::connect_async(&url) => r?,
    };
    let (mut sink, mut stream) = ws_stream.split();

    // 1. Wait for the relay's challenge (it speaks first).
    let challenge_nonce = wait_for_challenge(&mut stream).await?;

    // 2. Sign AUTH_DOMAIN ‖ 0x00 ‖ nonce and send the agent auth frame.
    let sig = crypto::sign_challenge(&state.identity().signing_key(), &challenge_nonce);
    let auth = serde_json::json!({
        "type": "auth",
        "role": "agent",
        "agent_ed25519_pub": hex::encode(state.identity().ed25519_pub()),
        "signature": hex::encode(sig),
    });
    sink.send(WsMessage::Text(auth.to_string().into())).await?;

    // 3. Expect auth_ok and verify the namespace_id locally.
    let ns = wait_for_auth_ok(&mut stream).await?;
    if ns != state.identity().namespace_id_hex() {
        return Err(anyhow!(
            "relay returned mismatched namespace_id (got {ns}, expected {})",
            state.identity().namespace_id_hex()
        ));
    }
    info!(plugin = "mobile-connector", "relay auth ok, namespace verified");
    state.set_connected(true);

    // 4. Send the current authorize set from the DB (empty on first run).
    let authorized = state.authorized_pubkeys_hex().await.unwrap_or_default();
    let authorize = serde_json::json!({ "type": "authorize", "clients": authorized });
    sink.send(WsMessage::Text(authorize.to_string().into())).await?;

    // 5. Main dispatch loop: outbound queue, inbound frames, keepalive.
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = sink.send(WsMessage::Close(None)).await;
                return Ok(());
            }

            // Outbound: frames queued by pairing / inbox broadcast / revoke.
            maybe = outbound_rx.recv() => {
                match maybe {
                    Some(text) => sink.send(WsMessage::Text(text.into())).await?,
                    None => return Ok(()), // channel closed → plugin stopping
                }
            }

            // Inbound: relay → agent frames.
            maybe = stream.next() => {
                let Some(msg) = maybe else { return Ok(()); }; // stream ended
                match msg? {
                    WsMessage::Text(txt) => {
                        if let Some(reply) = handle_incoming(state, &txt).await {
                            sink.send(WsMessage::Text(reply.into())).await?;
                        }
                    }
                    WsMessage::Ping(p) => sink.send(WsMessage::Pong(p)).await?,
                    WsMessage::Close(_) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

/// Read frames until a `challenge` arrives; returns the raw 32-byte nonce.
async fn wait_for_challenge(
    stream: &mut (impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Result<[u8; 32]> {
    while let Some(msg) = stream.next().await {
        if let WsMessage::Text(txt) = msg? {
            if let Ok(Outgoing::Challenge { nonce }) = serde_json::from_str::<Outgoing>(&txt) {
                return crypto::decode_hex::<32>(&nonce)
                    .ok_or_else(|| anyhow!("challenge nonce is not 32-byte hex"));
            }
        }
    }
    Err(anyhow!("connection closed before challenge"))
}

/// Read frames until `auth_ok`; returns the relay-reported namespace_id hex.
async fn wait_for_auth_ok(
    stream: &mut (impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Result<String> {
    while let Some(msg) = stream.next().await {
        if let WsMessage::Text(txt) = msg? {
            match serde_json::from_str::<Outgoing>(&txt) {
                Ok(Outgoing::AuthOk { namespace_id, .. }) => return Ok(namespace_id),
                Ok(Outgoing::AuthError { code, message }) => {
                    return Err(anyhow!("auth_error from relay: {code} ({message})"));
                }
                _ => {}
            }
        }
    }
    Err(anyhow!("connection closed before auth_ok"))
}

/// Dispatch one inbound relay→agent frame. Returns an optional text reply to
/// send (e.g. a `pong`). The heavy lifting (E2E decrypt, Inbox resolution,
/// pairing policy) is delegated to [`RelayState`].
async fn handle_incoming(state: &Arc<RelayState>, txt: &str) -> Option<String> {
    // Control frames first (challenge/auth/ping are typed in `Outgoing` here
    // because they are relay→agent; `Incoming` covers agent→relay frames the
    // relay parses, so we hand-match the relay's outgoing types).
    if let Ok(out) = serde_json::from_str::<Outgoing>(txt) {
        match out {
            Outgoing::Ping => return Some(r#"{"type":"pong"}"#.to_string()),
            Outgoing::Pong => return None,
            Outgoing::Message { from, nonce, ciphertext, .. } => {
                state.handle_inbound_message(&from, &nonce, &ciphertext).await;
                return None;
            }
            Outgoing::ClientPaired { client_ed25519_pub, client_x25519_pub, platform } => {
                state
                    .handle_client_paired(&client_ed25519_pub, &client_x25519_pub, &platform)
                    .await;
                return None;
            }
            Outgoing::AuthorizeOk { authorized } => {
                debug!(plugin = "mobile-connector", authorized, "authorize_ok");
                return None;
            }
            Outgoing::PairingReady { .. } | Outgoing::PairingStopOk => return None,
            Outgoing::Error { code, message } => {
                warn!(plugin = "mobile-connector", code, message, "relay error frame");
                return None;
            }
            _ => {}
        }
    }
    // Some relays may send a bare `{"type":"ping"}` that also parses as Incoming.
    if matches!(serde_json::from_str::<Incoming>(txt), Ok(Incoming::Ping)) {
        return Some(r#"{"type":"pong"}"#.to_string());
    }
    error!(plugin = "mobile-connector", "unrecognized relay frame dropped");
    None
}

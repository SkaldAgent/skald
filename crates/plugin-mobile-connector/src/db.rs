//! Persistence for authorized devices and their anti-replay counters
//! (plugin.md §9). The plugin does NOT open its own SQLite file: it reuses
//! Skald's shared `SqlitePool` from the `PluginContext` and namespaces its one
//! table with the `relay_` prefix.
//!
//! Counters MUST survive restarts (plugin.md §9 "⚠️"): a `send_counter` reset to
//! 0 would reuse an AES-GCM nonce under the same key, and a `recv_counter` reset
//! would re-open the replay window. So both are columns here, not in-memory.

use anyhow::Result;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

/// Authorization state of a paired device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Paired but not yet confirmed by the human (plugin.md §6).
    Pending,
    /// Confirmed — receives Inbox snapshots and may answer.
    Authorized,
}

impl ClientState {
    #[allow(dead_code)] // mirrors from_str; kept for completeness/debugging
    pub fn as_str(self) -> &'static str {
        match self {
            ClientState::Pending => "pending",
            ClientState::Authorized => "authorized",
        }
    }

    pub fn from_str(s: &str) -> ClientState {
        match s {
            "authorized" => ClientState::Authorized,
            _ => ClientState::Pending,
        }
    }
}

/// One row of `relay_clients`.
///
/// `send_counter` / `authorized_at` are part of the persisted schema (read back
/// for diagnostics / future use) even though the hot paths use the dedicated
/// counter helpers.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ClientRow {
    pub ed25519_pub: [u8; 32],
    pub x25519_pub: [u8; 32],
    pub state: ClientState,
    pub platform: Option<String>,
    /// Raw JSON of the `device_info` object received in `hello`.
    pub device_info: Option<String>,
    pub send_counter: u64,
    pub recv_counter: u64,
    pub authorized_at: Option<i64>,
    pub last_seen: Option<i64>,
}

/// Create the `relay_clients` table if missing (idempotent — called on reload).
pub async fn init(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS relay_clients (
            ed25519_pub   BLOB PRIMARY KEY,
            x25519_pub    BLOB NOT NULL,
            state         TEXT NOT NULL,
            platform      TEXT,
            device_info   TEXT,
            send_counter  INTEGER NOT NULL DEFAULT 0,
            recv_counter  INTEGER NOT NULL DEFAULT 0,
            authorized_at INTEGER,
            last_seen     INTEGER
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert (or replace) a freshly paired client with counters reset to 0 and
/// state = Pending (plugin.md §6 step 7c).
pub async fn upsert_paired(
    pool: &SqlitePool,
    ed25519_pub: &[u8; 32],
    x25519_pub: &[u8; 32],
    platform: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO relay_clients
            (ed25519_pub, x25519_pub, state, platform, send_counter, recv_counter)
         VALUES (?, ?, 'pending', ?, 0, 0)
         ON CONFLICT(ed25519_pub) DO UPDATE SET
            x25519_pub   = excluded.x25519_pub,
            state        = 'pending',
            platform     = excluded.platform,
            send_counter = 0,
            recv_counter = 0",
    )
    .bind(ed25519_pub.as_slice())
    .bind(x25519_pub.as_slice())
    .bind(platform)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark a client Authorized, stamping `authorized_at` with the current time.
pub async fn set_authorized(pool: &SqlitePool, ed25519_pub: &[u8; 32]) -> Result<()> {
    sqlx::query("UPDATE relay_clients SET state = 'authorized', authorized_at = ? WHERE ed25519_pub = ?")
        .bind(Utc::now().timestamp_millis())
        .bind(ed25519_pub.as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

/// Persist the device_info JSON received in a `hello` payload.
pub async fn set_device_info(pool: &SqlitePool, ed25519_pub: &[u8; 32], device_info_json: &str) -> Result<()> {
    sqlx::query("UPDATE relay_clients SET device_info = ?, last_seen = ? WHERE ed25519_pub = ?")
        .bind(device_info_json)
        .bind(Utc::now().timestamp_millis())
        .bind(ed25519_pub.as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

/// Atomically reserve the next send counter for a client and return it.
///
/// The new value is persisted BEFORE the caller seals/sends a message
/// (plugin.md §8): even if the process dies right after, the counter never
/// regresses, so no AES-GCM nonce is ever reused. Returns the counter value to
/// embed in the nonce.
pub async fn next_send_counter(pool: &SqlitePool, ed25519_pub: &[u8; 32]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let current: i64 =
        sqlx::query_scalar("SELECT send_counter FROM relay_clients WHERE ed25519_pub = ?")
            .bind(ed25519_pub.as_slice())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| anyhow::anyhow!("next_send_counter: client not found"))?;
    let next = current + 1;
    sqlx::query("UPDATE relay_clients SET send_counter = ? WHERE ed25519_pub = ?")
        .bind(next)
        .bind(ed25519_pub.as_slice())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(next as u64)
}

/// Persist a newly-seen receive counter after a valid `open` (plugin.md §8).
pub async fn set_recv_counter(pool: &SqlitePool, ed25519_pub: &[u8; 32], counter: u64) -> Result<()> {
    sqlx::query("UPDATE relay_clients SET recv_counter = ?, last_seen = ? WHERE ed25519_pub = ?")
        .bind(counter as i64)
        .bind(Utc::now().timestamp_millis())
        .bind(ed25519_pub.as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a client and all its derived state (keys/counters/device_info) on
/// revoke (plugin.md §7).
pub async fn delete(pool: &SqlitePool, ed25519_pub: &[u8; 32]) -> Result<()> {
    sqlx::query("DELETE FROM relay_clients WHERE ed25519_pub = ?")
        .bind(ed25519_pub.as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch one client by pubkey.
pub async fn get(pool: &SqlitePool, ed25519_pub: &[u8; 32]) -> Result<Option<ClientRow>> {
    let row = sqlx::query(
        "SELECT ed25519_pub, x25519_pub, state, platform, device_info,
                send_counter, recv_counter, authorized_at, last_seen
         FROM relay_clients WHERE ed25519_pub = ?",
    )
    .bind(ed25519_pub.as_slice())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_client))
}

/// List all clients.
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<ClientRow>> {
    let rows = sqlx::query(
        "SELECT ed25519_pub, x25519_pub, state, platform, device_info,
                send_counter, recv_counter, authorized_at, last_seen
         FROM relay_clients ORDER BY authorized_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_client).collect())
}

/// Hex pubkeys of all Authorized clients — the `authorize` set sent to the relay.
pub async fn authorized_pubkeys_hex(pool: &SqlitePool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT ed25519_pub FROM relay_clients WHERE state = 'authorized'")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let pk: Vec<u8> = r.get("ed25519_pub");
            hex::encode(pk)
        })
        .collect())
}

fn row_to_client(row: sqlx::sqlite::SqliteRow) -> ClientRow {
    let ed: Vec<u8> = row.get("ed25519_pub");
    let x: Vec<u8> = row.get("x25519_pub");
    let state: String = row.get("state");
    ClientRow {
        ed25519_pub: to_array(&ed),
        x25519_pub: to_array(&x),
        state: ClientState::from_str(&state),
        platform: row.get("platform"),
        device_info: row.get("device_info"),
        send_counter: row.get::<i64, _>("send_counter") as u64,
        recv_counter: row.get::<i64, _>("recv_counter") as u64,
        authorized_at: row.get("authorized_at"),
        last_seen: row.get("last_seen"),
    }
}

/// Convert a byte slice into a 32-byte array (zero-padded / truncated defensively).
fn to_array(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;

pub use crate::core::db::tool_permission_groups::ToolPermissionGroup;
use crate::core::approval::{ApprovalManager, RuleAction};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunContext {
    security_group:    Option<String>,
    #[serde(default)]
    pub system_prompt:     Vec<String>,
    #[serde(default)]
    pub allow_fs_writes:   Vec<String>,
    /// Working directory for tool calls. None means Skald's own process cwd.
    #[serde(default)]
    pub working_directory: Option<String>,
}

impl RunContext {
    pub fn with_security_group(security_group: Option<String>) -> Self {
        Self { security_group, ..Default::default() }
    }

    pub fn to_db(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn from_db(s: &str) -> Option<Self> {
        if s.is_empty() { return None; }
        serde_json::from_str(s).ok()
    }

    /// Permission group ID for approval rule lookup.
    pub fn tool_group_id(&self) -> Option<&str> {
        self.security_group.as_deref()
    }

    /// Combined system prompt fragments to inject as dynamic context, or None if empty.
    pub fn extra_system_prompt(&self) -> Option<String> {
        if self.system_prompt.is_empty() { return None; }
        Some(self.system_prompt.join("\n\n"))
    }

    /// Effective working directory for this session.
    /// Returns the configured path if set and non-empty, otherwise Skald's process cwd.
    pub fn effective_working_dir(&self) -> PathBuf {
        self.working_directory
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    /// True if writing to `path` is pre-authorized by this RunContext.
    /// Entries in `allow_fs_writes` are resolved against `effective_working_dir`,
    /// so relative entries like `"data"` are treated as relative to the session WD.
    /// Matching: exact file OR recursive directory prefix (no wildcards needed).
    pub fn is_write_allowed(&self, path: &str) -> bool {
        if self.allow_fs_writes.is_empty() { return false; }
        let wd  = self.effective_working_dir();
        let abs = make_absolute(path, &wd);
        self.allow_fs_writes.iter().any(|entry| {
            let e = make_absolute(entry, &wd);
            abs == e || abs.starts_with(&format!("{}/", e.trim_end_matches('/')))
        })
    }
}

/// Resolves `path` to absolute using `base` if relative; absolute paths are returned as-is (without trailing slash).
fn make_absolute(path: &str, base: &PathBuf) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        path.trim_end_matches('/').to_string()
    } else {
        base.join(path.trim_end_matches('/')).to_string_lossy().into_owned()
    }
}

pub struct RunContextManager {
    db:       Arc<SqlitePool>,
    approval: Arc<ApprovalManager>,
}

impl RunContextManager {
    pub fn new(db: Arc<SqlitePool>, approval: Arc<ApprovalManager>) -> Self {
        Self { db, approval }
    }

    /// Seeds the built-in "default" permission group and migrates legacy rules.
    /// Safe to call at every startup (idempotent).
    pub async fn seed_defaults(&self) -> Result<()> {
        crate::core::db::tool_permission_groups::insert_or_ignore(
            &self.db, "default", "Default", Some("Built-in default permission group"),
        ).await?;

        let migrated = sqlx::query("UPDATE approval_rules SET group_id = 'default' WHERE group_id IS NULL")
            .execute(self.db.as_ref())
            .await
            .map(|r| r.rows_affected())
            .unwrap_or(0);

        if migrated > 0 {
            info!(%migrated, "run_context: migrated approval rules to 'default' group");
        }

        Ok(())
    }

    // ── ToolPermissionGroup CRUD ───────────────────────────────────────────────

    pub async fn list_groups(&self) -> Result<Vec<ToolPermissionGroup>> {
        crate::core::db::tool_permission_groups::list(&self.db).await
    }

    pub async fn get_group(&self, id: &str) -> Result<Option<ToolPermissionGroup>> {
        crate::core::db::tool_permission_groups::get(&self.db, id).await
    }

    pub async fn create_group(
        &self,
        id:          &str,
        name:        &str,
        description: Option<&str>,
    ) -> Result<()> {
        if id == "default" {
            bail!("cannot create a permission group with reserved id 'default'");
        }
        crate::core::db::tool_permission_groups::insert(&self.db, id, name, description).await
    }

    pub async fn update_group(
        &self,
        id:          &str,
        name:        &str,
        description: Option<&str>,
    ) -> Result<bool> {
        crate::core::db::tool_permission_groups::update(&self.db, id, name, description).await
    }

    pub async fn delete_group(&self, id: &str) -> Result<bool> {
        if id == "default" {
            bail!("cannot delete the built-in 'default' permission group");
        }
        crate::core::db::tool_permission_groups::delete(&self.db, id).await
    }

    /// Duplicates a permission group and all its rules atomically.
    pub async fn duplicate_group(
        &self,
        source_id: &str,
        new_id:    &str,
        new_name:  &str,
    ) -> Result<()> {
        if new_id == "default" {
            bail!("cannot create a permission group with reserved id 'default'");
        }
        let source = crate::core::db::tool_permission_groups::get(&self.db, source_id).await?
            .ok_or_else(|| anyhow::anyhow!("source group '{source_id}' not found"))?;

        let mut tx = self.db.begin().await?;

        sqlx::query(
            "INSERT INTO tool_permission_groups (id, name, description) VALUES (?, ?, ?)",
        )
        .bind(new_id)
        .bind(new_name)
        .bind(source.description.as_deref())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO approval_rules \
                (agent_id, source, tool_pattern, path_pattern, action, note, priority, group_id) \
             SELECT agent_id, source, tool_pattern, path_pattern, action, note, priority, ? \
             FROM   approval_rules \
             WHERE  group_id = ?",
        )
        .bind(new_id)
        .bind(source_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    // ── Tool visibility ────────────────────────────────────────────────────────

    /// Returns the effective `RuleAction` for `tool_name` under the given permission group.
    /// `run_context_id` now directly holds a `tool_permission_groups` id (the run_contexts
    /// table indirection has been removed). Falls back to the `"default"` group when `None`.
    pub async fn check_tool_visibility(
        &self,
        run_context_id: Option<&str>,
        tool_name:      &str,
    ) -> Option<RuleAction> {
        let group_id = run_context_id.unwrap_or("default");
        self.approval.check_tool_visibility(group_id, tool_name).await
    }

    // ── Session assignment ─────────────────────────────────────────────────────

    /// Serialises `ctx` as JSON and stores it on the session row.
    /// `None` clears the context (falls back to the default permission group).
    pub async fn set_session_run_context(
        &self,
        session_id: i64,
        ctx:        Option<&RunContext>,
    ) -> Result<()> {
        let json = ctx.map(|rc| rc.to_db());
        sqlx::query("UPDATE chat_sessions SET run_context = ? WHERE id = ?")
            .bind(json.as_deref())
            .bind(session_id)
            .execute(self.db.as_ref())
            .await?;
        Ok(())
    }
}

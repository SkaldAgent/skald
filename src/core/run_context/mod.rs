use std::sync::Arc;

use anyhow::{Result, bail};
use sqlx::SqlitePool;
use tracing::info;

pub use crate::core::db::run_contexts::RunContextRow;
pub use crate::core::db::tool_permission_groups::ToolPermissionGroup;

pub struct RunContextManager {
    db: Arc<SqlitePool>,
}

impl RunContextManager {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Seeds the built-in "default" group and run_context if they don't exist yet,
    /// then migrates any legacy rules with NULL group_id to the "default" group.
    /// Safe to call at every startup (idempotent).
    pub async fn seed_defaults(&self) -> Result<()> {
        crate::core::db::tool_permission_groups::insert_or_ignore(
            &self.db, "default", "Default", Some("Built-in default permission group"),
        ).await?;

        crate::core::db::run_contexts::insert_or_ignore(
            &self.db, "default", "Default", Some("Built-in default run context"), Some("default"),
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

    // ── RunContext CRUD ────────────────────────────────────────────────────────

    pub async fn list_contexts(&self) -> Result<Vec<RunContextRow>> {
        crate::core::db::run_contexts::list(&self.db).await
    }

    pub async fn get_context(&self, id: &str) -> Result<Option<RunContextRow>> {
        crate::core::db::run_contexts::get(&self.db, id).await
    }

    pub async fn create_context(
        &self,
        id:            &str,
        name:          &str,
        description:   Option<&str>,
        tool_group_id: Option<&str>,
    ) -> Result<()> {
        if id == "default" {
            bail!("cannot create a run_context with reserved id 'default'");
        }
        crate::core::db::run_contexts::insert(&self.db, id, name, description, tool_group_id).await
    }

    pub async fn update_context(
        &self,
        id:            &str,
        name:          &str,
        description:   Option<&str>,
        tool_group_id: Option<&str>,
    ) -> Result<bool> {
        crate::core::db::run_contexts::update(&self.db, id, name, description, tool_group_id).await
    }

    pub async fn delete_context(&self, id: &str) -> Result<bool> {
        if id == "default" {
            bail!("cannot delete the built-in 'default' run_context");
        }
        crate::core::db::run_contexts::delete(&self.db, id).await
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

    // ── Session assignment ─────────────────────────────────────────────────────

    pub async fn set_session_run_context(
        &self,
        session_id:     i64,
        run_context_id: Option<&str>,
    ) -> Result<()> {
        crate::core::db::run_contexts::set_run_context_for_session(
            &self.db, session_id, run_context_id,
        ).await
    }
}

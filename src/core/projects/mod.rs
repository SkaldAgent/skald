pub mod tickets;

use std::sync::Arc;

use anyhow::Result;
use sqlx::SqlitePool;

use crate::core::db::projects::{self, Project};
use crate::core::run_context::RunContext;

pub struct ProjectManager {
    db: Arc<SqlitePool>,
}

impl ProjectManager {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<Project>> {
        projects::list(&self.db).await
    }

    pub async fn get(&self, id: i64) -> Result<Option<Project>> {
        projects::get(&self.db, id).await
    }

    pub async fn create(
        &self,
        name:        &str,
        path:        &str,
        description: &str,
        run_context: Option<&RunContext>,
    ) -> Result<Project> {
        let rc_json = run_context.map(|rc| rc.to_db());
        projects::create(&self.db, name, path, description, rc_json.as_deref()).await
    }

    pub async fn update(
        &self,
        id:          i64,
        name:        &str,
        path:        &str,
        description: &str,
        run_context: Option<&RunContext>,
    ) -> Result<bool> {
        let rc_json = run_context.map(|rc| rc.to_db());
        projects::update(&self.db, id, name, path, description, rc_json.as_deref()).await
    }

    pub async fn delete(&self, id: i64) -> Result<bool> {
        projects::delete(&self.db, id).await
    }
}

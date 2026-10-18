//! Instance destruction for the Postgres backend: the instance schema goes
//! with everything in it, and so does its registry row. Local files are a
//! separate step (`LocalDirCleanup`) composed in by the server.

use std::sync::Arc;

use async_trait::async_trait;
use memory_application::admin::InstanceLifecycle;
use memory_domain::error::DomainResult;
use memory_domain::scope::InstanceId;
use serde_json::{json, Value};

use super::db::PgDb;

pub struct PostgresCleanup {
    db: Arc<PgDb>,
}

impl PostgresCleanup {
    pub fn new(db: Arc<PgDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl InstanceLifecycle for PostgresCleanup {
    async fn destroy(&self, instance: &InstanceId) -> DomainResult<Value> {
        let removed = self.db.drop_instance(instance).await?;
        Ok(json!({"removed": removed}))
    }
}

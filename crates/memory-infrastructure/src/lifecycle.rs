//! Instance destruction composed of independent cleanup steps (storage,
//! files, scheduling state), so each backend contributes its own.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use memory_application::admin::InstanceLifecycle;
use memory_application::ports::pipeline::PipelineStateRepository;
use memory_domain::error::{DomainError, DomainResult};
use memory_domain::scope::InstanceId;
use serde_json::{json, Map, Value};

use crate::sqlite::db::InstancePool;

/// Runs every step and merges their reports into one JSON object.
pub struct CompositeLifecycle {
    steps: Vec<(&'static str, Arc<dyn InstanceLifecycle>)>,
}

impl CompositeLifecycle {
    pub fn new(steps: Vec<(&'static str, Arc<dyn InstanceLifecycle>)>) -> Self {
        Self { steps }
    }
}

#[async_trait]
impl InstanceLifecycle for CompositeLifecycle {
    async fn destroy(&self, instance: &InstanceId) -> DomainResult<Value> {
        let mut report = Map::new();
        for (name, step) in &self.steps {
            report.insert((*name).to_owned(), step.destroy(instance).await?);
        }
        Ok(Value::Object(report))
    }
}

/// Deletes `<data>/instances/<id>/` (files, and the SQLite database when
/// SQLite is the storage backend).
pub struct LocalDirCleanup {
    data_dir: PathBuf,
    sqlite: Option<Arc<InstancePool>>,
}

impl LocalDirCleanup {
    pub fn new(data_dir: PathBuf, sqlite: Option<Arc<InstancePool>>) -> Self {
        Self { data_dir, sqlite }
    }
}

#[async_trait]
impl InstanceLifecycle for LocalDirCleanup {
    async fn destroy(&self, instance: &InstanceId) -> DomainResult<Value> {
        if let Some(pool) = &self.sqlite {
            pool.evict(instance);
        }
        let dir = self.data_dir.join("instances").join(instance.as_str());
        let existed = dir.exists();
        if existed {
            tokio::fs::remove_dir_all(&dir).await.map_err(|e| {
                DomainError::unavailable(format!("failed to remove instance data: {e}"))
            })?;
        }
        Ok(json!({"removed": existed}))
    }
}

/// Forgets pipeline timers and cursors of the instance.
pub struct StateCleanup {
    state: Arc<dyn PipelineStateRepository>,
}

impl StateCleanup {
    pub fn new(state: Arc<dyn PipelineStateRepository>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl InstanceLifecycle for StateCleanup {
    async fn destroy(&self, instance: &InstanceId) -> DomainResult<Value> {
        self.state.delete_instance(instance).await?;
        Ok(json!({"removed": true}))
    }
}

//! Coordination between workers, possibly on several nodes.

use async_trait::async_trait;
use memory_domain::error::DomainResult;

/// Holds an exclusive lease until dropped. Implementations release the
/// lease in `Drop` (asynchronously if needed).
pub struct LockGuard {
    _lease: Box<dyn Send + Sync>,
}

impl LockGuard {
    pub fn new(lease: impl Send + Sync + 'static) -> Self {
        Self {
            _lease: Box::new(lease),
        }
    }
}

/// Mutual exclusion per key: in-process for a single node, distributed
/// (e.g. Redis) when several nodes share state.
#[async_trait]
pub trait KeyLock: Send + Sync {
    /// Waits until `key` is free, then holds it.
    async fn lock(&self, key: &str) -> DomainResult<LockGuard>;
    /// Holds `key` if it is free right now.
    async fn try_lock(&self, key: &str) -> DomainResult<Option<LockGuard>>;
}

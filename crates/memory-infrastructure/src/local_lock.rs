//! Single-node `KeyLock`: a fixed set of async mutexes picked by key hash
//! (bounded memory; keys sharing a stripe merely serialise).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use async_trait::async_trait;
use memory_application::ports::coordination::{KeyLock, LockGuard};
use memory_domain::error::DomainResult;
use tokio::sync::Mutex;

pub struct LocalKeyLock {
    stripes: Vec<Arc<Mutex<()>>>,
}

impl LocalKeyLock {
    pub fn new(stripes: usize) -> Self {
        Self {
            stripes: (0..stripes.max(1))
                .map(|_| Arc::new(Mutex::new(())))
                .collect(),
        }
    }

    fn stripe(&self, key: &str) -> Arc<Mutex<()>> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        Arc::clone(&self.stripes[(hasher.finish() % self.stripes.len() as u64) as usize])
    }
}

impl Default for LocalKeyLock {
    fn default() -> Self {
        Self::new(64)
    }
}

#[async_trait]
impl KeyLock for LocalKeyLock {
    async fn lock(&self, key: &str) -> DomainResult<LockGuard> {
        Ok(LockGuard::new(self.stripe(key).lock_owned().await))
    }

    async fn try_lock(&self, key: &str) -> DomainResult<Option<LockGuard>> {
        Ok(self.stripe(key).try_lock_owned().ok().map(LockGuard::new))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_held_key_cannot_be_taken_until_released() {
        let locks = LocalKeyLock::new(4);
        let guard = locks.lock("a").await.unwrap();
        assert!(locks.try_lock("a").await.unwrap().is_none());
        drop(guard);
        assert!(locks.try_lock("a").await.unwrap().is_some());
    }
}

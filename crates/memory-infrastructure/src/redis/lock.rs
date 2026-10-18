//! Distributed `KeyLock`: `SET key token NX PX ttl`, kept alive while held.
//!
//! A job can hold its serial key for minutes (LLM calls), longer than is
//! safe to set as a TTL for crashed holders. So the TTL stays short and a
//! keeper task renews it every `ttl / 3` while the guard lives; dropping
//! the guard stops the keeper, which then deletes the key if it still
//! owns it. A crashed node stops renewing and its locks expire after `ttl`.

use std::sync::Arc;
use std::time::Duration;

use ::redis::aio::ConnectionManager;
use ::redis::{cmd, Script};
use async_trait::async_trait;
use memory_application::ports::coordination::{KeyLock, LockGuard};
use memory_domain::error::DomainResult;
use rand::Rng;
use tokio::sync::oneshot;
use tracing::warn;

use super::{unavailable, KeySpace};

const RETRY_MIN: Duration = Duration::from_millis(5);
const RETRY_MAX: Duration = Duration::from_millis(250);

/// Extends the TTL only if we still own the key.
const RENEW: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('PEXPIRE', KEYS[1], ARGV[2])
end
return 0
"#;

/// Deletes the key only if we still own it (never someone else's lock).
const RELEASE: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('DEL', KEYS[1])
end
return 0
"#;

struct Shared {
    conn: ConnectionManager,
    ttl: Duration,
    renew: Script,
    release: Script,
}

pub struct RedisKeyLock {
    shared: Arc<Shared>,
    keys: KeySpace,
}

impl RedisKeyLock {
    /// `namespace` separates lock spaces (state vs job locks) that may
    /// otherwise use equal keys.
    pub fn new(conn: ConnectionManager, namespace: KeySpace, ttl: Duration) -> Self {
        Self {
            shared: Arc::new(Shared {
                conn,
                ttl: ttl.max(Duration::from_millis(30)),
                renew: Script::new(RENEW),
                release: Script::new(RELEASE),
            }),
            keys: namespace,
        }
    }
}

/// Held inside the `LockGuard`; dropping it closes the channel, which tells
/// the keeper to stop renewing and release.
struct Lease {
    _stop: oneshot::Sender<()>,
}

fn ttl_ms(ttl: Duration) -> u64 {
    u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX)
}

/// Renews `key` until the guard drops, then releases it.
async fn keep(shared: Arc<Shared>, key: String, token: String, mut stop: oneshot::Receiver<()>) {
    let mut ticker = tokio::time::interval(shared.ttl / 3);
    ticker.tick().await; // the first tick is immediate
    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = ticker.tick() => {
                let renewed: Result<i64, _> = shared
                    .renew
                    .key(&key)
                    .arg(&token)
                    .arg(ttl_ms(shared.ttl))
                    .invoke_async(&mut shared.conn.clone())
                    .await;
                match renewed {
                    Ok(1) => {}
                    Ok(_) => {
                        warn!(key, "lock expired before renewal; another holder may run");
                        break;
                    }
                    // Transient: the next tick retries well before expiry.
                    Err(err) => warn!(key, error = %err, "lock renewal failed"),
                }
            }
        }
    }
    let released: Result<i64, _> = shared
        .release
        .key(&key)
        .arg(&token)
        .invoke_async(&mut shared.conn.clone())
        .await;
    if let Err(err) = released {
        warn!(key, error = %err, "lock release failed; it expires after its ttl");
    }
}

#[async_trait]
impl KeyLock for RedisKeyLock {
    async fn lock(&self, key: &str) -> DomainResult<LockGuard> {
        let mut backoff = RETRY_MIN;
        loop {
            if let Some(guard) = self.try_lock(key).await? {
                return Ok(guard);
            }
            // Jitter keeps contending nodes from retrying in lockstep.
            let jitter = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64 / 2);
            tokio::time::sleep(backoff + Duration::from_millis(jitter)).await;
            backoff = (backoff * 2).min(RETRY_MAX);
        }
    }

    async fn try_lock(&self, key: &str) -> DomainResult<Option<LockGuard>> {
        let key = self.keys.key(key);
        let token = uuid::Uuid::new_v4().to_string();
        let set: Option<String> = cmd("SET")
            .arg(&key)
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(ttl_ms(self.shared.ttl))
            .query_async(&mut self.shared.conn.clone())
            .await
            .map_err(unavailable)?;
        if set.is_none() {
            return Ok(None);
        }
        let (tx, rx) = oneshot::channel();
        tokio::spawn(keep(Arc::clone(&self.shared), key, token, rx));
        Ok(Some(LockGuard::new(Lease { _stop: tx })))
    }
}

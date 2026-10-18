//! Connection pool, error mapping and lazily migrated per-instance schemas.
//!
//! Every instance lives in its own schema, `am_<16 hex of sha256(id)>`:
//! instance ids are arbitrary strings (up to 256 bytes), schema names must
//! be short identifiers, and a hash keeps the mapping deterministic across
//! nodes. The registry table `agent_memory.instances` records which
//! instance owns which schema, so a (practically impossible) hash collision
//! surfaces as a unique violation instead of two tenants sharing tables.
//! All SQL names tables fully qualified (`"am_…".table`) — `search_path` is
//! per connection and a pool cannot be trusted to carry it.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use memory_domain::error::{DomainError, DomainResult};
use memory_domain::scope::InstanceId;
use sha2::{Digest, Sha256};
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Postgres, Transaction};
use tracing::{info, warn};

use super::schema::{self, HNSW_MAX_DIMS, REGISTRY_SCHEMA};
use super::sql::{args, count, execute, execute_script, fetch_optional, get, Arg};

/// How long a request waits for a pooled connection (and how long start-up
/// waits for the first one) before reporting storage as unavailable.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn pg_err(e: sqlx::Error) -> DomainError {
    match &e {
        // Class 23 = integrity constraint violation, like SQLite's
        // `ConstraintViolation`.
        sqlx::Error::Database(db) if db.code().is_some_and(|c| c.starts_with("23")) => {
            DomainError::conflict("duplicate_entry", e.to_string())
        }
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => {
            DomainError::unavailable(format!("postgres unreachable: {e}"))
        }
        _ => DomainError::unavailable(format!("storage error: {e}")),
    }
}

/// Double-quotes an identifier.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The schema name owned by `instance`.
pub fn schema_name(instance: &InstanceId) -> String {
    let digest = Sha256::digest(instance.as_str().as_bytes());
    format!("am_{}", &hex::encode(digest)[..16])
}

/// A migrated instance schema; displays as its quoted name for use in SQL.
#[derive(Debug, Clone)]
pub struct Schema(Arc<str>);

impl fmt::Display for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Takes a transaction-scoped advisory lock named `key`.
pub async fn xact_lock(tx: &mut Transaction<'_, Postgres>, key: &str) -> DomainResult<()> {
    execute(
        tx,
        "SELECT pg_advisory_xact_lock(hashtextextended(?, 0))",
        &args![key],
    )
    .await
    .map(|_| ())
}

/// The shared pool plus the set of instance schemas known to be migrated.
pub struct PgDb {
    pool: PgPool,
    dims: Option<usize>,
    /// pgvector ≥ 0.8 can keep scanning HNSW until enough rows pass the
    /// scope filter; older versions may return fewer than `limit` rows.
    iterative_scan: bool,
    ready: RwLock<HashMap<InstanceId, Schema>>,
    /// Serialises first-use migrations within this process (the advisory
    /// lock serialises them across processes).
    migrating: tokio::sync::Mutex<()>,
}

impl PgDb {
    /// Connects (failing fast when the server is unreachable), installs
    /// pgvector when embeddings are on, and creates the registry.
    pub async fn connect(
        url: &str,
        max_connections: u32,
        dims: Option<usize>,
    ) -> DomainResult<Self> {
        let options = PgConnectOptions::from_str(url)
            .map_err(|e| DomainError::invalid(format!("storage.postgres.url: {e}")))?;
        let target = format!(
            "{}:{}/{}",
            options.get_host(),
            options.get_port(),
            options.get_database().unwrap_or("")
        );
        let unreachable = |e: &dyn fmt::Display| {
            DomainError::unavailable(format!("cannot connect to postgres at {target}: {e}"))
        };
        // One direct attempt first: the pool retries refused connections
        // until its acquire timeout, which would only delay a clear error.
        let probe = tokio::time::timeout(ACQUIRE_TIMEOUT, PgConnection::connect_with(&options))
            .await
            .map_err(|e| unreachable(&e))?
            .map_err(|e| unreachable(&e))?;
        probe.close().await.map_err(pg_err)?;
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.max(1))
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_with(options)
            .await
            .map_err(|e| unreachable(&e))?;
        let mut tx = pool.begin().await.map_err(pg_err)?;
        xact_lock(&mut tx, "agent_memory:bootstrap").await?;
        if dims.is_some() {
            execute(&mut tx, "CREATE EXTENSION IF NOT EXISTS vector", &[])
                .await
                .map_err(|e| {
                    DomainError::unavailable(format!(
                        "pgvector is required when embeddings are enabled ({}); install it or run `CREATE EXTENSION vector` as a privileged user",
                        e.detail
                    ))
                })?;
        }
        execute_script(
            &mut tx,
            &schema::registry_ddl(&quote_ident(REGISTRY_SCHEMA)),
        )
        .await?;
        let version: Option<String> = match fetch_optional(
            &mut tx,
            "SELECT extversion FROM pg_extension WHERE extname = 'vector'",
            &[],
        )
        .await?
        {
            Some(row) => Some(get(&row, "extversion")?),
            None => None,
        };
        tx.commit().await.map_err(pg_err)?;
        info!(%target, pgvector = ?version, "connected to postgres");
        Ok(Self {
            pool,
            dims,
            iterative_scan: version.as_deref().is_some_and(supports_iterative_scan),
            ready: RwLock::new(HashMap::new()),
            migrating: tokio::sync::Mutex::new(()),
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn begin(&self) -> DomainResult<Transaction<'static, Postgres>> {
        self.pool.begin().await.map_err(pg_err)
    }

    /// A pooled connection for a single statement or a short sequence.
    pub async fn conn(&self) -> DomainResult<PoolConnection<Postgres>> {
        self.pool.acquire().await.map_err(pg_err)
    }

    pub fn dims(&self) -> Option<usize> {
        self.dims
    }

    pub fn iterative_scan(&self) -> bool {
        self.iterative_scan
    }

    /// `pgvector-hnsw`, `pgvector-exact` (too many dimensions for HNSW) or
    /// `none`, for `/health`.
    pub fn vector_index(&self) -> &'static str {
        match self.dims {
            Some(d) if d <= HNSW_MAX_DIMS => "pgvector-hnsw",
            Some(_) => "pgvector-exact",
            None => "none",
        }
    }

    fn cached(&self, instance: &InstanceId) -> Option<Schema> {
        self.ready.read().ok()?.get(instance).cloned()
    }

    /// The instance's schema, created and migrated on first use.
    pub async fn schema(&self, instance: &InstanceId) -> DomainResult<Schema> {
        if let Some(s) = self.cached(instance) {
            return Ok(s);
        }
        let _guard = self.migrating.lock().await;
        if let Some(s) = self.cached(instance) {
            return Ok(s);
        }
        let name = schema_name(instance);
        let quoted = quote_ident(&name);
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        xact_lock(&mut tx, &format!("agent_memory:migrate:{name}")).await?;
        let registry = quote_ident(REGISTRY_SCHEMA);
        execute(
            &mut tx,
            &format!("INSERT INTO {registry}.instances (instance_id, schema_name) VALUES (?, ?) ON CONFLICT (instance_id) DO NOTHING"),
            &args![instance.as_str(), name.as_str()],
        )
        .await?;
        execute_script(&mut tx, &schema::instance_ddl(&quoted)).await?;
        if let Some(dims) = self.dims {
            migrate_embedding(&mut tx, &quoted, dims).await?;
        }
        tx.commit().await.map_err(pg_err)?;
        let schema = Schema(quoted.into());
        if let Ok(mut ready) = self.ready.write() {
            ready.insert(instance.clone(), schema.clone());
        }
        Ok(schema)
    }

    /// Drops the instance schema and its registry row. Returns whether the
    /// instance was registered.
    pub async fn drop_instance(&self, instance: &InstanceId) -> DomainResult<bool> {
        let _guard = self.migrating.lock().await;
        if let Ok(mut ready) = self.ready.write() {
            ready.remove(instance);
        }
        let name = schema_name(instance);
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        xact_lock(&mut tx, &format!("agent_memory:migrate:{name}")).await?;
        execute(
            &mut tx,
            &format!("DROP SCHEMA IF EXISTS {} CASCADE", quote_ident(&name)),
            &[],
        )
        .await?;
        let registry = quote_ident(REGISTRY_SCHEMA);
        let removed = execute(
            &mut tx,
            &format!("DELETE FROM {registry}.instances WHERE instance_id = ?"),
            &args![instance.as_str()],
        )
        .await?;
        tx.commit().await.map_err(pg_err)?;
        Ok(removed > 0)
    }

    /// Instances registered in the database.
    pub async fn known_instances(&self) -> DomainResult<Vec<String>> {
        let registry = quote_ident(REGISTRY_SCHEMA);
        let rows = super::sql::fetch_all(
            &mut *self.conn().await?,
            &format!("SELECT instance_id FROM {registry}.instances ORDER BY instance_id"),
            &[],
        )
        .await?;
        rows.iter().map(|r| get(r, "instance_id")).collect()
    }
}

fn supports_iterative_scan(version: &str) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= (0, 8)
}

/// Adds the `vector(dims)` column and index. When the configured
/// dimensionality changed, stored embeddings came from another model and
/// cannot be compared with new queries, so they are cleared.
async fn migrate_embedding(
    tx: &mut Transaction<'_, Postgres>,
    schema: &str,
    dims: usize,
) -> DomainResult<()> {
    execute(tx, &schema::embedding_column_ddl(schema, dims), &[]).await?;
    let current = count(
        tx,
        "SELECT atttypmod::bigint FROM pg_attribute WHERE attrelid = (?)::regclass AND attname = 'embedding'",
        &[Arg::from(format!("{schema}.atoms"))],
    )
    .await?;
    if current != dims {
        warn!(%schema, from = current, to = dims, "embedding dimensions changed; clearing stored embeddings");
        execute(
            tx,
            &format!("DROP INDEX IF EXISTS {schema}.ix_atoms_embedding"),
            &[],
        )
        .await?;
        execute(
            tx,
            &format!(
                "ALTER TABLE {schema}.atoms ALTER COLUMN embedding TYPE vector({dims}) USING NULL"
            ),
            &[],
        )
        .await?;
    }
    if dims <= HNSW_MAX_DIMS {
        execute(tx, &schema::embedding_index_ddl(schema), &[]).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_domain::scope::SegmentId;

    #[test]
    fn schema_names_are_short_safe_identifiers() {
        let id = SegmentId::parse("instance", "Weird \"name\"; DROP").unwrap();
        let name = schema_name(&id);
        assert_eq!(name.len(), 19);
        assert!(name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        assert_eq!(name, schema_name(&id), "deterministic");
    }

    #[test]
    fn quotes_identifiers() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn detects_iterative_scan_support() {
        assert!(supports_iterative_scan("0.8.0"));
        assert!(!supports_iterative_scan("0.7.4"));
        assert!(supports_iterative_scan("1.0"));
    }
}

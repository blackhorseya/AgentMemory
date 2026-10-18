//! Optional sqlite-vec (`vec0`) KNN index over `atoms.embedding`.
//!
//! The `atoms.embedding` BLOB stays the source of truth; `atoms_vec` is a
//! derived index keyed by `atoms.seq` (a rewrite gets a new seq, so the
//! old index row is dropped with the old atom row). The index records its
//! dimension in `index_meta`; a missing or different value means the index
//! cannot be trusted and is rebuilt from the BLOBs on next use. Writers
//! running without the index clear that value, so an index left behind by
//! an earlier configuration is never served stale.

use std::ffi::{c_char, c_int};
use std::sync::{Once, OnceLock};

use rusqlite::ffi::{sqlite3, sqlite3_api_routines, sqlite3_auto_extension, SQLITE_OK};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use super::sql::Where;

/// `index_meta` key holding the dimension `atoms_vec` was built with.
const DIMS_KEY: &str = "atoms_vec_dims";

/// Upper bound sqlite-vec accepts for `k` in a KNN query.
pub const MAX_K: usize = 4096;

type EntryPoint =
    unsafe extern "C" fn(*mut sqlite3, *mut *mut c_char, *const sqlite3_api_routines) -> c_int;

static REGISTER: Once = Once::new();
static REGISTERED: OnceLock<bool> = OnceLock::new();
static AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Registers sqlite-vec as an auto extension so every connection opened
/// afterwards (by any rusqlite user in the process) has `vec0`. Runs once.
pub fn register() {
    REGISTER.call_once(|| {
        // SAFETY: `sqlite3_vec_init` is the extension entry point compiled
        // with SQLITE_CORE; SQLite calls it with exactly this signature.
        let rc = unsafe {
            let init: EntryPoint = std::mem::transmute::<unsafe extern "C" fn(), EntryPoint>(
                sqlite_vec::sqlite3_vec_init,
            );
            sqlite3_auto_extension(Some(init))
        };
        let _ = REGISTERED.set(rc == SQLITE_OK);
        if rc != SQLITE_OK {
            tracing::warn!(
                rc,
                "sqlite-vec registration failed; using brute-force vectors"
            );
        }
    });
}

/// Whether `vec0` works in this process (probed once on a scratch DB).
pub fn available() -> bool {
    *AVAILABLE.get_or_init(|| {
        register();
        REGISTERED.get().copied().unwrap_or(false)
            && Connection::open_in_memory()
                .and_then(|c| c.query_row("SELECT vec_version()", [], |r| r.get::<_, String>(0)))
                .is_ok()
    })
}

/// A live `vec0` index of a fixed dimension.
#[derive(Debug, Clone, Copy)]
pub struct VecIndex {
    dims: usize,
}

/// Isolation and [`AtomFilter`](memory_application::ports::memory::AtomFilter)
/// predicates, all evaluated inside the KNN scan.
pub struct KnnFilter {
    pub team_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub atom_type: Option<String>,
    pub updated_from_ms: Option<i64>,
    pub updated_to_ms: Option<i64>,
}

impl VecIndex {
    /// `Some` when dims are known and the extension loaded.
    pub fn new(dims: Option<usize>) -> Option<Self> {
        dims.filter(|d| *d > 0 && available())
            .map(|dims| Self { dims })
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    /// Makes `atoms_vec` match the configured dimension, rebuilding it from
    /// the BLOBs if it is missing or was built for another dimension. Call
    /// inside the caller's transaction: the pool may reopen or recreate the
    /// database file at any time, so this is checked per operation (one
    /// primary-key lookup) rather than cached.
    pub fn ensure(&self, conn: &Connection) -> rusqlite::Result<()> {
        if stored_dims(conn)? == Some(self.dims as i64) {
            return Ok(());
        }
        tracing::info!(dims = self.dims, "rebuilding sqlite-vec atom index");
        conn.execute_batch(&format!(
            "DROP TABLE IF EXISTS atoms_vec;
             CREATE VIRTUAL TABLE atoms_vec USING vec0(
                 team_id TEXT PARTITION KEY,
                 embedding float[{dims}] distance_metric=cosine,
                 agent_id TEXT,
                 user_id TEXT,
                 atom_type TEXT,
                 updated_at_ms INTEGER
             );",
            dims = self.dims
        ))?;
        conn.execute(
            "INSERT INTO atoms_vec (rowid, team_id, embedding, agent_id, user_id, atom_type, updated_at_ms) \
             SELECT seq, team_id, embedding, agent_id, user_id, atom_type, updated_at_ms FROM atoms \
             WHERE embedding IS NOT NULL AND length(embedding) = ?",
            [self.blob_len()],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO index_meta (key, value) VALUES (?, ?)",
            params![DIMS_KEY, self.dims as i64],
        )?;
        Ok(())
    }

    /// Indexes the atom row `seq` (already written to `atoms`). Embeddings of
    /// another dimension stay in the BLOB but are not indexed.
    pub fn insert(&self, conn: &Connection, seq: i64) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO atoms_vec (rowid, team_id, embedding, agent_id, user_id, atom_type, updated_at_ms) \
             SELECT seq, team_id, embedding, agent_id, user_id, atom_type, updated_at_ms FROM atoms \
             WHERE seq = ? AND embedding IS NOT NULL AND length(embedding) = ?",
            params![seq, self.blob_len()],
        )?;
        Ok(())
    }

    /// Drops index rows for these atom sequences. vec0 deletes by rowid
    /// only efficiently, so rows go one at a time.
    pub fn remove(&self, conn: &Connection, seqs: &[i64]) -> rusqlite::Result<()> {
        let mut stmt = conn.prepare_cached("DELETE FROM atoms_vec WHERE rowid = ?")?;
        for seq in seqs {
            stmt.execute([seq])?;
        }
        Ok(())
    }

    /// The `k` nearest atom sequences with their cosine distance, nearest
    /// first. All filters run inside the vec0 scan, so `k` results are the
    /// true top-k of the caller's scope.
    pub fn knn(
        &self,
        conn: &Connection,
        query: &[f32],
        filter: &KnnFilter,
        k: usize,
    ) -> rusqlite::Result<Vec<(i64, f64)>> {
        let mut w = Where::default();
        w.raw(
            "embedding MATCH ?",
            [Value::from(super::sql::embedding_to_blob(query))],
        )
        .raw("k = ?", [Value::from(k.min(MAX_K) as i64)])
        .eq("team_id", filter.team_id.clone())
        .eq("agent_id", filter.agent_id.clone())
        .eq("user_id", filter.user_id.clone())
        .eq_opt("atom_type", filter.atom_type.clone());
        if let Some(from) = filter.updated_from_ms {
            w.raw("updated_at_ms >= ?", [Value::from(from)]);
        }
        if let Some(to) = filter.updated_to_ms {
            w.raw("updated_at_ms <= ?", [Value::from(to)]);
        }
        let sql = format!(
            "SELECT rowid, distance FROM atoms_vec {} ORDER BY distance",
            w.sql()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(w.params.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect();
        rows
    }

    fn blob_len(&self) -> i64 {
        (self.dims * std::mem::size_of::<f32>()) as i64
    }
}

/// Marks any existing index as untrusted. Used by writers running without
/// the index (dims unknown or extension missing) so a later run with the
/// index enabled rebuilds instead of serving rows that missed these writes.
pub fn invalidate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM index_meta WHERE key = ?", [DIMS_KEY])?;
    Ok(())
}

/// Dimension recorded for the current index, if any.
pub fn stored_dims(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT value FROM index_meta WHERE key = ?",
        [DIMS_KEY],
        |r| r.get(0),
    )
    .optional()
}

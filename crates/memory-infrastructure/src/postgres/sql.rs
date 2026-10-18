//! Query building for Postgres: SQL is written with `?` placeholders (as in
//! the SQLite adapter) and numbered `$1..$n` just before execution, so the
//! filter builders compose without tracking parameter positions.

use std::future::Future;
use std::pin::Pin;

use memory_domain::error::{DomainError, DomainResult};
use pgvector::Vector;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::postgres::{PgArguments, PgConnection, PgRow};
use sqlx::types::Json;
use sqlx::{Arguments, Row};

use super::db::pg_err;
use crate::text::tokenize;

/// One bound parameter.
#[derive(Debug, Clone)]
pub enum Arg {
    Text(Option<String>),
    Int(Option<i64>),
    Bool(bool),
    Texts(Vec<String>),
    Json(serde_json::Value),
    Vector(Option<Vector>),
}

impl From<String> for Arg {
    fn from(v: String) -> Self {
        Self::Text(Some(v))
    }
}

impl From<&str> for Arg {
    fn from(v: &str) -> Self {
        Self::Text(Some(v.to_owned()))
    }
}

impl From<&String> for Arg {
    fn from(v: &String) -> Self {
        Self::Text(Some(v.clone()))
    }
}

impl From<Option<String>> for Arg {
    fn from(v: Option<String>) -> Self {
        Self::Text(v)
    }
}

impl From<i64> for Arg {
    fn from(v: i64) -> Self {
        Self::Int(Some(v))
    }
}

impl From<Option<i64>> for Arg {
    fn from(v: Option<i64>) -> Self {
        Self::Int(v)
    }
}

impl From<u32> for Arg {
    fn from(v: u32) -> Self {
        Self::Int(Some(i64::from(v)))
    }
}

impl From<usize> for Arg {
    fn from(v: usize) -> Self {
        Self::Int(Some(i64::try_from(v).unwrap_or(i64::MAX)))
    }
}

impl From<bool> for Arg {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

/// Builds an argument array: `args![a, b, c]`.
macro_rules! args {
    ($($v:expr),* $(,)?) => {
        [$($crate::postgres::sql::Arg::from($v)),*]
    };
}
pub(crate) use args;

/// Rewrites `?` placeholders as `$1, $2, …`. Statements never contain a
/// literal `?` (no JSONB `?` operators, no quoted question marks).
pub fn numbered(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 16);
    let mut n = 0;
    for c in sql.chars() {
        if c == '?' {
            n += 1;
            out.push('$');
            out.push_str(&n.to_string());
        } else {
            out.push(c);
        }
    }
    out
}

fn arguments(args: &[Arg]) -> DomainResult<PgArguments> {
    let mut out = PgArguments::default();
    for a in args {
        let added = match a {
            Arg::Text(v) => out.add(v.clone()),
            Arg::Int(v) => out.add(*v),
            Arg::Bool(v) => out.add(*v),
            Arg::Texts(v) => out.add(v.clone()),
            Arg::Json(v) => out.add(Json(v.clone())),
            Arg::Vector(v) => out.add(v.clone()),
        };
        added.map_err(|e| DomainError::internal(format!("bind parameter: {e}")))?;
    }
    Ok(out)
}

/// Futures are boxed as `Send` here, at a concrete lifetime: left
/// generic, sqlx's executor bounds become higher-ranked inside
/// `async_trait` futures and rustc cannot prove them.
pub type DbFuture<'a, T> = Pin<Box<dyn Future<Output = DomainResult<T>> + Send + 'a>>;

pub fn execute<'a>(e: &'a mut PgConnection, sql: &'a str, args: &'a [Arg]) -> DbFuture<'a, u64> {
    Box::pin(async move {
        let sql = numbered(sql);
        sqlx::query_with(&sql, arguments(args)?)
            .execute(e)
            .await
            .map(|r| r.rows_affected())
            .map_err(pg_err)
    })
}

pub fn fetch_all<'a>(
    e: &'a mut PgConnection,
    sql: &'a str,
    args: &'a [Arg],
) -> DbFuture<'a, Vec<PgRow>> {
    Box::pin(async move {
        let sql = numbered(sql);
        sqlx::query_with(&sql, arguments(args)?)
            .fetch_all(e)
            .await
            .map_err(pg_err)
    })
}

pub fn fetch_optional<'a>(
    e: &'a mut PgConnection,
    sql: &'a str,
    args: &'a [Arg],
) -> DbFuture<'a, Option<PgRow>> {
    Box::pin(async move {
        let sql = numbered(sql);
        sqlx::query_with(&sql, arguments(args)?)
            .fetch_optional(e)
            .await
            .map_err(pg_err)
    })
}

/// Runs several `;`-separated statements without parameters (DDL).
pub fn execute_script<'a>(e: &'a mut PgConnection, sql: &'a str) -> DbFuture<'a, ()> {
    Box::pin(async move {
        sqlx::Executor::execute(e, sqlx::raw_sql(sql))
            .await
            .map(|_| ())
            .map_err(pg_err)
    })
}

/// `SELECT COUNT(*) …` (or any single BIGINT) as a `usize`.
pub fn count<'a>(e: &'a mut PgConnection, sql: &'a str, args: &'a [Arg]) -> DbFuture<'a, usize> {
    Box::pin(async move {
        let sql = numbered(sql);
        let n: i64 = sqlx::query_scalar_with(&sql, arguments(args)?)
            .fetch_one(e)
            .await
            .map_err(pg_err)?;
        Ok(usize::try_from(n).unwrap_or(0))
    })
}

pub fn get<'r, T>(row: &'r PgRow, column: &str) -> DomainResult<T>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_err(pg_err)
}

/// Deserialises a JSONB document column.
pub fn doc<T: DeserializeOwned>(row: &PgRow, column: &str) -> DomainResult<T> {
    let Json(value): Json<T> = row.try_get(column).map_err(pg_err)?;
    Ok(value)
}

pub fn to_doc<T: Serialize>(value: &T) -> DomainResult<Arg> {
    serde_json::to_value(value)
        .map(Arg::Json)
        .map_err(|e| DomainError::internal(format!("serialize document: {e}")))
}

/// Every row's `doc` column.
pub fn docs<T: DeserializeOwned>(rows: &[PgRow]) -> DomainResult<Vec<T>> {
    rows.iter().map(|r| doc(r, "doc")).collect()
}

/// Accumulates `AND`-ed predicates with `?` parameters.
#[derive(Default, Clone)]
pub struct Where {
    clauses: Vec<String>,
    pub params: Vec<Arg>,
}

impl Where {
    pub fn eq(&mut self, column: &str, value: impl Into<Arg>) -> &mut Self {
        self.clauses.push(format!("{column} = ?"));
        self.params.push(value.into());
        self
    }

    pub fn eq_opt<V: Into<Arg>>(&mut self, column: &str, value: Option<V>) -> &mut Self {
        if let Some(v) = value {
            self.eq(column, v);
        }
        self
    }

    pub fn raw(&mut self, clause: &str, params: impl IntoIterator<Item = Arg>) -> &mut Self {
        self.clauses.push(clause.to_owned());
        self.params.extend(params);
        self
    }

    /// `column = ANY(?)`; an empty list matches nothing, like SQLite's `IN ()`.
    pub fn in_list(&mut self, column: &str, values: &[String]) -> &mut Self {
        if values.is_empty() {
            self.clauses.push("FALSE".into());
        } else {
            self.clauses.push(format!("{column} = ANY(?)"));
            self.params.push(Arg::Texts(values.to_vec()));
        }
        self
    }

    pub fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }

    pub fn with(&self, extra: impl IntoIterator<Item = Arg>) -> Vec<Arg> {
        let mut params = self.params.clone();
        params.extend(extra);
        params
    }
}

/// Positions above this are clamped by Postgres anyway.
const MAX_POSITION: usize = 16_383;

fn lexeme(token: &str) -> String {
    format!("'{}'", token.replace('\\', "\\\\").replace('\'', "''"))
}

/// A `tsvector` literal of [`tokenize`]'s tokens with their positions.
///
/// The tokens are cast verbatim (`?::tsvector`) instead of going through
/// `to_tsvector`: the text-search parser would re-split them (`rust_lang`
/// → `rust`, `lang`) and, under a non-UTF-8 `LC_CTYPE`, drop CJK
/// characters. Lower-casing — all the `simple` configuration does — has
/// already happened in `tokenize`, so this equals `simple` without the
/// locale dependence. Positions feed `ts_rank_cd`'s cover density.
pub fn tsvector_literal(text: &str) -> String {
    tokenize(text)
        .iter()
        .take(MAX_POSITION)
        .enumerate()
        .map(|(i, t)| format!("{}:{}", lexeme(t), i + 1))
        .collect::<Vec<_>>()
        .join(" ")
}

/// An OR-ed `tsquery` literal of the query's distinct, non-stop-word
/// tokens (the same selection as `text::match_query`). `None` when nothing
/// searchable remains.
pub fn tsquery_literal(query: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let terms: Vec<String> = tokenize(query)
        .into_iter()
        .filter(|t| !crate::text::is_stop_word(t))
        .filter(|t| seen.insert(t.clone()))
        .map(|t| lexeme(&t))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_placeholders_in_order() {
        assert_eq!(
            numbered("a = ? AND b = ANY(?) LIMIT ?"),
            "a = $1 AND b = ANY($2) LIMIT $3"
        );
    }

    #[test]
    fn tsvector_keeps_tokens_verbatim_with_positions() {
        assert_eq!(
            tsvector_literal("用Rust_x 記憶體"),
            "'用':1 'rust_x':2 '記憶':3 '憶體':4"
        );
    }

    #[test]
    fn tsquery_ors_distinct_tokens_and_drops_stop_words() {
        assert_eq!(
            tsquery_literal("the release Release plan").unwrap(),
            "'release' | 'plan'"
        );
        assert_eq!(tsquery_literal("?!"), None);
    }
}

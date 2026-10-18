//! Value types shared by use cases and ports.

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimeRange {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

impl TimeRange {
    pub fn contains(&self, t: DateTime<Utc>) -> bool {
        self.start.is_none_or(|s| t >= s) && self.end.is_none_or(|e| t <= e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub limit: usize,
    pub offset: usize,
}

impl Page {
    pub const DEFAULT_LIMIT: usize = 20;
    pub const MAX_LIMIT: usize = 100;

    /// Clamps caller input to `1..=max`, defaulting to `default`.
    pub fn clamped(
        limit: Option<usize>,
        offset: Option<usize>,
        default: usize,
        max: usize,
    ) -> Self {
        Self {
            limit: limit.unwrap_or(default).clamp(1, max),
            offset: offset.unwrap_or(0),
        }
    }

    pub fn standard(limit: Option<usize>, offset: Option<usize>) -> Self {
        Self::clamped(limit, offset, Self::DEFAULT_LIMIT, Self::MAX_LIMIT)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Scored<T> {
    pub item: T,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listed<T> {
    pub items: Vec<T>,
    pub total: usize,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct BatchDeleteResult {
    pub deleted_ids: Vec<String>,
    pub failed: Vec<FailedId>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FailedId {
    pub id: String,
    pub reason: String,
}

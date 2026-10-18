//! Retrieval policies: reciprocal-rank fusion of keyword and vector hits and
//! the budget that keeps recalled memory from flooding the context window.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Standard RRF damping constant.
pub const DEFAULT_RRF_K: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Bm25,
    Embedding,
    #[default]
    Hybrid,
}

impl SearchMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "bm25" | "keyword" => Some(Self::Bm25),
            "embedding" | "vector" => Some(Self::Embedding),
            "hybrid" => Some(Self::Hybrid),
            _ => None,
        }
    }
}

/// One ranked hit from a single retriever. Lists passed to [`rrf_fuse`]
/// must be ordered best-first.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedHit {
    pub id: String,
    pub score: f64,
}

/// Reciprocal Rank Fusion: `score(d) = Σ 1 / (k + rank_i(d))`, rank 1-based.
/// Rank-based fusion avoids having to calibrate BM25 against cosine scores.
pub fn rrf_fuse(lists: &[Vec<RankedHit>], k: f64) -> Vec<RankedHit> {
    let mut fused: HashMap<&str, f64> = HashMap::new();
    let mut first_seen: Vec<&str> = Vec::new();
    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            let entry = fused.entry(hit.id.as_str()).or_insert_with(|| {
                first_seen.push(hit.id.as_str());
                0.0
            });
            *entry += 1.0 / (k + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<RankedHit> = first_seen
        .into_iter()
        .map(|id| RankedHit {
            id: id.to_owned(),
            score: fused[id],
        })
        .collect();
    // Stable sort keeps first-seen order for ties, so results are deterministic.
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// Normalises a fused score into 0..=1 relative to the best possible score
/// (rank 1 in every list), so a single `score_threshold` works for any mode.
pub fn normalise_rrf(score: f64, list_count: usize, k: f64) -> f64 {
    if list_count == 0 {
        return 0.0;
    }
    let best = list_count as f64 / (k + 1.0);
    (score / best).clamp(0.0, 1.0)
}

/// Upper bounds on what recall may inject into a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecallBudget {
    pub max_items: usize,
    pub max_chars: usize,
}

/// Keeps items in order until either budget is exhausted. An item that does
/// not fit is skipped (not truncated) so recalled facts stay intact; a later
/// shorter item may still fit.
pub fn apply_budget<T>(items: Vec<T>, budget: RecallBudget, len: impl Fn(&T) -> usize) -> Vec<T> {
    let mut used = 0usize;
    let mut kept = Vec::new();
    for item in items {
        if kept.len() >= budget.max_items {
            break;
        }
        let size = len(&item);
        if used + size > budget.max_chars {
            continue;
        }
        used += size;
        kept.push(item);
    }
    kept
}

/// Cosine similarity; returns 0 for mismatched or zero vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(ids: &[&str]) -> Vec<RankedHit> {
        ids.iter()
            .map(|id| RankedHit {
                id: (*id).into(),
                score: 1.0,
            })
            .collect()
    }

    #[test]
    fn rrf_rewards_documents_found_by_both_retrievers() {
        let fused = rrf_fuse(&[hits(&["a", "b", "c"]), hits(&["c", "a"])], DEFAULT_RRF_K);
        let order: Vec<_> = fused.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(order, ["a", "c", "b"]);
    }

    #[test]
    fn normalised_top_score_is_one() {
        let fused = rrf_fuse(&[hits(&["a"]), hits(&["a"])], DEFAULT_RRF_K);
        let n = normalise_rrf(fused[0].score, 2, DEFAULT_RRF_K);
        assert!((n - 1.0).abs() < 1e-9);
    }

    #[test]
    fn budget_skips_oversized_items_but_keeps_smaller_later_ones() {
        let items = vec!["aaaa", "bbbbbbbbbb", "cc"];
        let kept = apply_budget(
            items,
            RecallBudget {
                max_items: 5,
                max_chars: 7,
            },
            |s| s.len(),
        );
        assert_eq!(kept, vec!["aaaa", "cc"]);
    }

    #[test]
    fn cosine_of_identical_vectors_is_one() {
        assert!((cosine(&[1.0, 2.0], &[1.0, 2.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }
}

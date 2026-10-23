//! Hybrid search: reciprocal rank fusion of the BM25 and semantic rankings.
//!
//! BM25 ranks micro-chunks while the semantic index ranks the parent windows
//! that contain them, so the two lists only share an identity once each BM25
//! chunk is treated as a hit on its parent window(s). Fusion therefore runs
//! over window ids and the result is a list of parent-window hits, the same
//! unit `search semantic` returns.

use crate::{
    archive::Archive,
    config::KatokConfig,
    search::{bm25_chunk_ids, hydrate_parent_hits},
    semantic::semantic_search_live_with_config,
    types::SearchHit,
    Error, Result,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// Standard RRF constant (Cormack et al.); damps the head of each list.
const RRF_K: f64 = 60.0;
/// Each input contributes `limit * CANDIDATE_MULTIPLIER` candidates so a window
/// ranked just below `limit` by one input can still be lifted by the other.
const CANDIDATE_MULTIPLIER: usize = 3;

/// Fuses BM25 and semantic rankings of `query` into parent-window hits.
///
/// Needs a current semantic index: its failures (`SemanticIndexMissing`,
/// `SemanticIndexStale`, ...) are returned unchanged rather than degrading to a
/// keyword-only ranking that would still be labelled hybrid. An input with no
/// matches leaves the other input's ranking.
pub async fn hybrid_search_with_config(
    archive: &Archive,
    semantic_dir: &Path,
    query: &str,
    limit: usize,
    config: &KatokConfig,
) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Err(Error::EmptyQuery);
    }
    let pool = limit.saturating_mul(CANDIDATE_MULTIPLIER);
    let semantic_windows =
        semantic_search_live_with_config(archive, semantic_dir, query, pool, config)
            .await?
            .into_iter()
            .enumerate()
            .map(|(index, hit)| (hit.chunk_id, index + 1))
            .collect::<Vec<_>>();
    let mut chunk_windows = Vec::new();
    for chunk_id in bm25_chunk_ids(archive, query, pool)? {
        chunk_windows.push(archive.window_parent_ids(&chunk_id)?);
    }
    let lexical_windows = lift_to_windows(chunk_windows);
    let mut ids = reciprocal_rank_fusion(&[lexical_windows, semantic_windows]);
    ids.truncate(limit);
    hydrate_parent_hits(archive, ids, "hybrid", query, config.snippet_length)
}

/// Turns a ranked list of chunks, each given as the windows containing it, into
/// a ranked list of windows. A window keeps the rank of its best chunk, and the
/// windows a chunk spans share that rank. A chunk in no window takes no rank, as
/// the semantic index cannot return it, and neither does a chunk whose windows
/// were all ranked by an earlier chunk.
fn lift_to_windows(chunk_windows: Vec<Vec<String>>) -> Vec<(String, usize)> {
    let mut seen = HashSet::new();
    let mut lifted = Vec::new();
    let mut rank = 0;
    for windows in chunk_windows {
        let mut fresh = windows
            .into_iter()
            .filter(|window| seen.insert(window.clone()))
            .peekable();
        if fresh.peek().is_some() {
            rank += 1;
            lifted.extend(fresh.map(|window| (window, rank)));
        }
    }
    lifted
}

/// Reciprocal rank fusion over ranked `(id, rank)` lists (best first, ranks from
/// 1; equal ranks are ties). Ids repeated in one list count once, at their best
/// rank. Ties keep first-seen order, so earlier lists win.
fn reciprocal_rank_fusion(lists: &[Vec<(String, usize)>]) -> Vec<String> {
    let mut scores: Vec<(&str, f64)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for list in lists {
        let mut seen = HashSet::new();
        for (id, rank) in list {
            if !seen.insert(id.as_str()) {
                continue;
            }
            let gain = 1.0 / (RRF_K + *rank as f64);
            match index.get(id.as_str()) {
                Some(&slot) => scores[slot].1 += gain,
                None => {
                    index.insert(id.as_str(), scores.len());
                    scores.push((id.as_str(), gain));
                }
            }
        }
    }
    // `sort_by` is stable, so equal scores stay in first-seen order.
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    scores.into_iter().map(|(id, _)| id.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::{lift_to_windows, reciprocal_rank_fusion};

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn ranked(values: &[(&str, usize)]) -> Vec<(String, usize)> {
        values
            .iter()
            .map(|(value, rank)| (value.to_string(), *rank))
            .collect()
    }

    /// A ranking with no ties: the first id is rank 1, the next rank 2, ...
    fn in_order(values: &[&str]) -> Vec<(String, usize)> {
        ids(values).into_iter().zip(1..).collect()
    }

    #[test]
    fn an_id_both_rankings_return_outranks_ids_only_one_returns() {
        let fused = reciprocal_rank_fusion(&[in_order(&["a", "b"]), in_order(&["c", "b"])]);

        assert_eq!(fused, ids(&["b", "a", "c"]));
    }

    #[test]
    fn ties_keep_first_seen_order_so_the_first_list_wins() {
        let fused = reciprocal_rank_fusion(&[in_order(&["a"]), in_order(&["b"])]);

        assert_eq!(fused, ids(&["a", "b"]));
    }

    #[test]
    fn an_id_repeated_inside_one_list_counts_once_at_its_best_rank() {
        // "a" repeated at ranks 1 and 2 must not out-score "b", which both lists return at rank 2.
        let fused = reciprocal_rank_fusion(&[in_order(&["a", "a", "b"]), in_order(&["c", "b"])]);

        assert_eq!(fused, ids(&["b", "a", "c"]));
    }

    #[test]
    fn an_empty_ranking_leaves_the_other_ranking_untouched() {
        let fused = reciprocal_rank_fusion(&[Vec::new(), in_order(&["x", "y"])]);

        assert_eq!(fused, ids(&["x", "y"]));
    }

    #[test]
    fn chunks_of_one_window_count_once_at_the_best_chunk_rank() {
        let lifted = lift_to_windows(vec![
            ids(&["w1"]),
            ids(&["w1"]),
            ids(&["w2"]),
            ids(&["w1", "w3"]),
        ]);

        assert_eq!(lifted, in_order(&["w1", "w2", "w3"]));
    }

    #[test]
    fn windows_sharing_one_chunk_share_its_rank() {
        let lifted = lift_to_windows(vec![ids(&["w1", "w2"]), ids(&["w3"])]);

        assert_eq!(lifted, ranked(&[("w1", 1), ("w2", 1), ("w3", 2)]));
    }

    #[test]
    fn a_chunk_split_across_windows_does_not_push_later_bm25_windows_down() {
        // w3 holds part of the best BM25 chunk and is third in the semantic ranking;
        // w4 holds the second BM25 chunk and is second in the semantic ranking.
        let lexical = lift_to_windows(vec![ids(&["w1", "w2", "w3"]), ids(&["w4"])]);
        let semantic = ranked(&[("w9", 1), ("w4", 2), ("w3", 3)]);

        let fused = reciprocal_rank_fusion(&[lexical, semantic]);

        assert_eq!(fused, ids(&["w3", "w4", "w1", "w2", "w9"]));
    }

    #[test]
    fn a_chunk_in_no_window_is_skipped_without_shifting_later_ranks() {
        let lifted = lift_to_windows(vec![Vec::new(), ids(&["w2"])]);

        assert_eq!(lifted, in_order(&["w2"]));
    }
}

//! Rank fusion.
//!
//! Different retrievers produce scores on incomparable scales (BM25 is
//! unbounded, cosine is `[-1,1]`, edit distance is `[0,1]`). Reciprocal Rank
//! Fusion sidesteps normalisation entirely by combining *ranks*, which is both
//! simpler and more robust than tuning per-retriever score mappings.

use std::collections::HashMap;

use yk_core::query::MatchSource;

/// Standard RRF damping constant.
const K: f32 = 60.0;

pub struct RankedList {
    pub source: MatchSource,
    pub weight: f32,
    /// Ordered best-first.
    pub ids: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct Fused {
    pub id: i64,
    pub score: f32,
    pub sources: Vec<MatchSource>,
}

pub fn fuse(lists: &[RankedList]) -> Vec<Fused> {
    let mut acc: HashMap<i64, (f32, Vec<MatchSource>)> = HashMap::new();

    for list in lists {
        for (rank, id) in list.ids.iter().enumerate() {
            let entry = acc.entry(*id).or_insert((0.0, Vec::new()));
            entry.0 += list.weight / (K + rank as f32 + 1.0);
            if !entry.1.contains(&list.source) {
                entry.1.push(list.source);
            }
        }
    }

    let mut out: Vec<Fused> = acc
        .into_iter()
        .map(|(id, (score, sources))| Fused { id, score, sources })
        .collect();
    // Ties broken by id so results are deterministic across runs.
    out.sort_unstable_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(source: MatchSource, weight: f32, ids: &[i64]) -> RankedList {
        RankedList { source, weight, ids: ids.to_vec() }
    }

    #[test]
    fn agreement_between_retrievers_wins() {
        let fused = fuse(&[
            list(MatchSource::Keyword, 1.0, &[1, 2, 3]),
            list(MatchSource::Semantic, 1.0, &[9, 2, 8]),
        ]);
        // 2 is ranked second by both; everything else appears in one list only.
        assert_eq!(fused[0].id, 2);
        assert_eq!(fused[0].sources.len(), 2);
    }

    #[test]
    fn weights_bias_the_outcome() {
        let low = fuse(&[
            list(MatchSource::Keyword, 0.1, &[1]),
            list(MatchSource::Semantic, 1.0, &[2]),
        ]);
        assert_eq!(low[0].id, 2);
    }

    #[test]
    fn single_list_preserves_order() {
        let fused = fuse(&[list(MatchSource::Fuzzy, 1.0, &[5, 4, 3])]);
        assert_eq!(fused.iter().map(|f| f.id).collect::<Vec<_>>(), vec![5, 4, 3]);
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert!(fuse(&[]).is_empty());
        assert!(fuse(&[list(MatchSource::Keyword, 1.0, &[])]).is_empty());
    }

    #[test]
    fn is_deterministic_on_ties() {
        let a = fuse(&[list(MatchSource::Keyword, 1.0, &[7, 3])]);
        let b = fuse(&[list(MatchSource::Keyword, 1.0, &[7, 3])]);
        assert_eq!(a[0].id, b[0].id);
    }
}

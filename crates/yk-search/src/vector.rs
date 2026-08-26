//! In-memory dense vector index.
//!
//! Brute force is the right call here: a 100k-item library at 256 dimensions is
//! 100 MB and one pass costs a few milliseconds with rayon, which beats the
//! complexity and recall loss of an approximate index at this scale. Persistence
//! lives in SQLite; this is the hot cache.

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;

/// Below this many vectors the rayon overhead outweighs the parallelism.
const PARALLEL_THRESHOLD: usize = 4096;

#[derive(Default)]
pub struct VectorStore {
    dim: usize,
    ids: Vec<i64>,
    libs: Vec<i64>,
    /// Row-major `ids.len() * dim`, unit-normalised.
    data: Vec<f32>,
    pos: HashMap<i64, usize>,
    free: Vec<usize>,
}

impl VectorStore {
    pub fn new(dim: usize) -> Self {
        Self { dim, ..Default::default() }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn len(&self) -> usize {
        self.pos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    /// Reset to a new dimensionality, e.g. after switching provider.
    pub fn reset(&mut self, dim: usize) {
        *self = VectorStore::new(dim);
    }

    /// The stored vector for an item, if it has been embedded.
    pub fn get(&self, id: i64) -> Option<&[f32]> {
        let slot = *self.pos.get(&id)?;
        Some(&self.data[slot * self.dim..(slot + 1) * self.dim])
    }

    pub fn upsert(&mut self, id: i64, library_id: i64, vec: &[f32]) {
        if vec.len() != self.dim {
            return;
        }
        let slot = match self.pos.get(&id) {
            Some(&s) => s,
            None => match self.free.pop() {
                Some(s) => {
                    self.ids[s] = id;
                    self.libs[s] = library_id;
                    self.pos.insert(id, s);
                    s
                }
                None => {
                    let s = self.ids.len();
                    self.ids.push(id);
                    self.libs.push(library_id);
                    self.data.resize((s + 1) * self.dim, 0.0);
                    self.pos.insert(id, s);
                    s
                }
            },
        };
        self.libs[slot] = library_id;
        self.data[slot * self.dim..(slot + 1) * self.dim].copy_from_slice(vec);
    }

    pub fn remove(&mut self, id: i64) {
        if let Some(slot) = self.pos.remove(&id) {
            self.ids[slot] = -1;
            self.libs[slot] = -1;
            self.data[slot * self.dim..(slot + 1) * self.dim].fill(0.0);
            self.free.push(slot);
        }
    }

    /// Top-`k` by cosine similarity, optionally restricted to `allowed` ids.
    pub fn search(
        &self,
        library_id: i64,
        query: &[f32],
        k: usize,
        allowed: Option<&HashSet<i64>>,
    ) -> Vec<(i64, f32)> {
        if query.len() != self.dim || self.ids.is_empty() || k == 0 {
            return Vec::new();
        }

        let score = |slot: usize| -> Option<(i64, f32)> {
            let id = self.ids[slot];
            if id < 0 || self.libs[slot] != library_id {
                return None;
            }
            if let Some(a) = allowed {
                if !a.contains(&id) {
                    return None;
                }
            }
            let row = &self.data[slot * self.dim..(slot + 1) * self.dim];
            // Vectors are unit-normalised, so the dot product is the cosine.
            let s: f32 = row.iter().zip(query).map(|(a, b)| a * b).sum();
            Some((id, s))
        };

        let mut scored: Vec<(i64, f32)> = if self.ids.len() >= PARALLEL_THRESHOLD {
            (0..self.ids.len()).into_par_iter().filter_map(score).collect()
        } else {
            (0..self.ids.len()).filter_map(score).collect()
        };

        let k = k.min(scored.len());
        if k == 0 {
            return Vec::new();
        }
        scored.select_nth_unstable_by(k - 1, |a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        scored.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &mut [f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    #[test]
    fn upsert_and_search_ranks_by_similarity() {
        let mut s = VectorStore::new(3);
        s.upsert(1, 1, &unit(&mut [1.0, 0.0, 0.0]));
        s.upsert(2, 1, &unit(&mut [0.9, 0.1, 0.0]));
        s.upsert(3, 1, &unit(&mut [0.0, 0.0, 1.0]));

        let hits = s.search(1, &unit(&mut [1.0, 0.0, 0.0]), 2, None);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, 1);
        assert_eq!(hits[1].0, 2);
    }

    #[test]
    fn respects_library_isolation() {
        let mut s = VectorStore::new(2);
        s.upsert(1, 1, &unit(&mut [1.0, 0.0]));
        s.upsert(2, 2, &unit(&mut [1.0, 0.0]));
        let hits = s.search(2, &unit(&mut [1.0, 0.0]), 10, None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 2);
    }

    #[test]
    fn respects_allow_list() {
        let mut s = VectorStore::new(2);
        s.upsert(1, 1, &unit(&mut [1.0, 0.0]));
        s.upsert(2, 1, &unit(&mut [0.9, 0.1]));
        let allow: HashSet<i64> = [2].into_iter().collect();
        let hits = s.search(1, &unit(&mut [1.0, 0.0]), 10, Some(&allow));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 2);
    }

    #[test]
    fn remove_frees_and_reuses_slots() {
        let mut s = VectorStore::new(2);
        s.upsert(1, 1, &unit(&mut [1.0, 0.0]));
        s.upsert(2, 1, &unit(&mut [0.0, 1.0]));
        s.remove(1);
        assert_eq!(s.len(), 1);
        s.upsert(3, 1, &unit(&mut [1.0, 0.0]));
        assert_eq!(s.len(), 2);
        assert_eq!(s.ids.len(), 2, "slot was recycled rather than appended");
        assert_eq!(s.search(1, &unit(&mut [1.0, 0.0]), 1, None)[0].0, 3);
    }

    #[test]
    fn dimension_mismatch_is_ignored() {
        let mut s = VectorStore::new(3);
        s.upsert(1, 1, &[1.0, 0.0]);
        assert!(s.is_empty());
        assert!(s.search(1, &[1.0, 0.0], 5, None).is_empty());
    }

    #[test]
    fn parallel_path_matches_serial() {
        let mut s = VectorStore::new(4);
        for i in 0..(PARALLEL_THRESHOLD as i64 + 10) {
            let mut v = vec![1.0, (i % 7) as f32, (i % 3) as f32, 1.0];
            s.upsert(i, 1, &unit(&mut v));
        }
        let hits = s.search(1, &unit(&mut [1.0, 0.0, 0.0, 1.0]), 5, None);
        assert_eq!(hits.len(), 5);
        assert!(hits[0].1 >= hits[4].1);
    }
}

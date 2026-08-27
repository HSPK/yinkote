//! Turning text into vectors.
//!
//! Two implementations ship in the box:
//!
//! * [`LocalEmbedder`] — a deterministic hashed n-gram projection. Offline,
//!   instant, no dependencies. It captures lexical and morphological
//!   similarity (including CJK) rather than true semantics, and exists so that
//!   semantic search *always works* out of the box — a feature that only works
//!   once somebody signs up for an API key is a feature most people never see.
//! * [`OpenAiEmbedder`] — any OpenAI-compatible `/embeddings` endpoint
//!   (OpenAI, DeepSeek, Qwen, Ollama, vLLM …) for real semantic vectors.

use async_trait::async_trait;
use crate::provider::EmbeddingProvider;
use yk_core::{text, Error, Result};

pub const LOCAL_DIM: usize = 256;

/// Deterministic offline embedder using the hashing trick with signed buckets.
#[derive(Clone)]
pub struct LocalEmbedder {
    dim: usize,
}

impl LocalEmbedder {
    pub fn new() -> Self {
        Self { dim: LOCAL_DIM }
    }

    fn hash(token: &str, seed: u64) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
        for b in token.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    fn encode(&self, input: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];

        let mut add = |token: &str, weight: f32| {
            let h = Self::hash(token, 0);
            let idx = (h % self.dim as u64) as usize;
            // A second hash decides the sign, which halves collision damage.
            let sign = if Self::hash(token, 0x9E37_79B9) & 1 == 0 { 1.0 } else { -1.0 };
            v[idx] += weight * sign;
        };

        for tok in text::tokenize(input) {
            // Rarer (longer) tokens carry more signal.
            add(&tok, 1.0 + (tok.chars().count() as f32).min(8.0) * 0.1);
        }
        // Character trigrams give robustness to morphology and typos.
        for tri in text::trigrams(input) {
            add(&tri, 0.35);
        }

        normalize(&mut v);
        v
    }
}

impl Default for LocalEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingProvider for LocalEmbedder {
    fn id(&self) -> &str {
        "local-hash"
    }

    fn dimensions(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.encode(t)).collect())
    }
}

/// OpenAI-compatible embeddings endpoint.
pub struct OpenAiEmbedder {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
    dim: usize,
    id: String,
}

impl OpenAiEmbedder {
    pub fn new(endpoint: &str, model: &str, api_key: Option<String>, dim: usize) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
            dim,
            id: format!("remote:{model}"),
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn dimensions(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Same treatment as a chat call: an embedding pass is a background job
        // over thousands of items, so giving up on one rate limit means the
        // whole batch has to be found and redone later.
        let resp = crate::retry::send(|| {
            let mut req = self
                .client
                .post(format!("{}/embeddings", self.endpoint))
                .json(&serde_json::json!({ "model": self.model, "input": texts }));
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            req
        })
        .await
        .map_err(|e| Error::Unavailable(format!("embeddings request failed: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Unavailable(format!("embeddings {status}: {body}")));
        }

        #[derive(serde::Deserialize)]
        struct Row {
            embedding: Vec<f32>,
            #[serde(default)]
            index: usize,
        }
        #[derive(serde::Deserialize)]
        struct Body {
            data: Vec<Row>,
        }

        let mut body: Body =
            resp.json().await.map_err(|e| Error::Unavailable(format!("bad embeddings body: {e}")))?;
        body.data.sort_by_key(|r| r.index);
        Ok(body
            .data
            .into_iter()
            .map(|mut r| {
                normalize(&mut r.embedding);
                r.embedding
            })
            .collect())
    }
}

/// Scale to unit length so cosine similarity reduces to a dot product.
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn vec_of(s: &str) -> Vec<f32> {
        LocalEmbedder::new().embed(&[s.to_string()]).await.unwrap().remove(0)
    }

    fn dot(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[tokio::test]
    async fn is_deterministic_and_unit_length() {
        let a = vec_of("diffusion models for molecules").await;
        let b = vec_of("diffusion models for molecules").await;
        assert_eq!(a, b);
        assert!((dot(&a, &a) - 1.0).abs() < 1e-4);
    }

    #[tokio::test]
    async fn related_text_scores_higher_than_unrelated() {
        let q = vec_of("diffusion model molecule generation").await;
        let near = vec_of("generating molecules with diffusion models").await;
        let far = vec_of("a history of medieval agriculture").await;
        assert!(dot(&q, &near) > dot(&q, &far));
    }

    #[tokio::test]
    async fn works_for_chinese() {
        let q = vec_of("扩散模型 分子生成").await;
        let near = vec_of("用扩散模型做分子生成的研究").await;
        let far = vec_of("宋代农业经济史").await;
        assert!(dot(&q, &near) > dot(&q, &far));
    }

    #[tokio::test]
    async fn tolerates_typos() {
        let q = vec_of("attention is all you need").await;
        let typo = vec_of("attension is all you ned").await;
        let far = vec_of("bananas are yellow").await;
        assert!(dot(&q, &typo) > dot(&q, &far));
    }
}

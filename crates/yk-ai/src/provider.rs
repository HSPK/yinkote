//! The contract every model service is reduced to.

use async_trait::async_trait;
use yk_core::Result;

use crate::stream::{ignore, Sink};
use crate::types::{ChatMessage, ChatRequest};

#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// A short name for the model, shown in the UI.
    fn model(&self) -> String;

    /// Answer, reporting fragments as they arrive.
    ///
    /// Streaming is the *primary* method rather than an optional extra: a turn
    /// that shows nothing for half a minute is indistinguishable from one that
    /// has hung, and making the honest version the default one means a provider
    /// has to opt out rather than forget to opt in.
    async fn complete(&self, request: ChatRequest, on_delta: Sink<'_>) -> Result<ChatMessage>;

    /// Answer, and never mind the fragments.
    async fn answer(&self, request: ChatRequest) -> Result<ChatMessage> {
        self.complete(request, &ignore).await
    }
}

/// Turning text into vectors.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Identifies the provider *and* its model, because a vector is only
    /// comparable to another from the same one — the id is what stops a
    /// changed model quietly poisoning the index.
    fn id(&self) -> &str;
    fn dimensions(&self) -> usize;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

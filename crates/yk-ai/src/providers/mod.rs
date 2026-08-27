//! One module per dialect.
//!
//! A provider's only job is translation: take the shapes in `types`, say them
//! the way this service expects, and turn what comes back into the same shapes
//! and the events in `stream`. Everything provider-specific stops here.

mod embeddings;
mod openai;

pub use embeddings::{LocalEmbedder, OpenAiEmbedder, LOCAL_DIM};
pub use openai::{OpenAiConfig, OpenAiProvider};

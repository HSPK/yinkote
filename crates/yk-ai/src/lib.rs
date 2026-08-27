//! Everything about talking to a model.
//!
//! One crate owns the whole of it: the message shapes, the provider contract,
//! the streaming events, and the dialect each service actually speaks. Before
//! this, the types lived in `yk-core`, the chat dialect in `yk-server` and the
//! embedding dialect in `yk-search` — three places that all had to agree, none
//! of which could be tested without dragging in a database or an HTTP server.
//!
//! The shape follows the same reasoning `pi-ai` arrived at: providers differ in
//! their wire format and in nothing else that matters, so their differences are
//! normalised at the edge into one set of events, and everything above works
//! against the contract rather than against a service.
//!
//! Nothing here knows about libraries, items or SQLite. The only thing it
//! borrows from the rest of the project is the error type, so a failure to
//! reach a model reads like every other failure.

pub mod provider;
pub mod retry;
pub mod providers;
pub mod stream;
pub mod types;

pub use provider::{ChatProvider, EmbeddingProvider};
pub use providers::{LocalEmbedder, OpenAiConfig, OpenAiEmbedder, OpenAiProvider};
pub use stream::{Delta, Sink};
pub use types::{ChatMessage, ChatRequest, Tool, ToolCall, ToolSpec};

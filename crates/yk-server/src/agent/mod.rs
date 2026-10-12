//! Agent wiring: a chat provider and the set of tools it is given.
//!
//! The provider speaks the OpenAI chat-completions dialect, which is what
//! local runners (Ollama, llama.cpp, vLLM, LM Studio) and hosted services alike
//! expose — so "which model" is a URL and a name in the config file rather than
//! a code change.
//!
//! Business capabilities are discovered over MCP from the REST operation
//! catalogue. This module only owns provider wiring, reading projections,
//! workspace access and skills.

use yk_ai::{OpenAiConfig, OpenAiProvider};

use crate::config::AgentConfig;

/// Point a provider at whatever the config names.
///
/// The dialect itself lives in `yk-ai`; this only translates one config shape
/// into another, which is all a composition root should ever do.
pub fn provider(config: &AgentConfig) -> yk_core::Result<OpenAiProvider> {
    OpenAiProvider::new(&OpenAiConfig {
        endpoint: config.endpoint.clone().unwrap_or_default(),
        model: config.model.clone().unwrap_or_default(),
        api_key: config.api_key.clone(),
        timeout_secs: config.timeout_secs,
    })
}

pub mod skills;
pub mod workspace;
pub use workspace::Workspace;

pub mod reading;
pub use reading::{summarise, SYSTEM_PROMPT};

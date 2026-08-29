//! Agent wiring: a chat provider and the set of tools it is given.
//!
//! The provider speaks the OpenAI chat-completions dialect, which is what
//! local runners (Ollama, llama.cpp, vLLM, LM Studio) and hosted services alike
//! expose — so "which model" is a URL and a name in the config file rather than
//! a code change.
//!
//! One file per kind of power, and this one holds none of them: `reading` is
//! what the assistant may look at, `actions` what it may change, `workspace`
//! what it may touch outside the library, `skills` what it may be taught. So
//! "what can this thing do to my data" is answerable by opening one file
//! rather than by reading past a provider constructor to find out.

use std::sync::Arc;

use yk_ai::{OpenAiConfig, OpenAiProvider, Tool};
use yk_core::ports::SearchIndex;
use yk_store::Store;

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

pub mod actions;
pub mod skills;
pub mod workspace;
pub use actions::{Action, LibraryAction, ACTIONS};
pub use workspace::Workspace;

pub mod reading;
pub use reading::{
    summarise, GetItem, LibraryOverview, ListReferences, ReadPaper, SearchLibrary, SYSTEM_PROMPT,
};


/// Every tool that could exist, whether or not it is switched on.
///
/// The settings page needs this: a list of only the *enabled* tools cannot
/// offer to re-enable one, and a second hand-written list would drift from
/// the first the moment a tool is added.
pub fn tool_catalogue(
    store: &Store,
    search: &Arc<dyn SearchIndex>,
    scrape: &Arc<yk_scrape::ScrapeEngine>,
    outside: &Arc<yk_scrape::search::SearchEngine>,
    workspace: Option<&Workspace>,
    skills: &Arc<yk_agent::skills::Skills>,
) -> Vec<String> {
    let mut names: Vec<String> = tools(store, search, scrape, outside).iter().map(|t| t.spec().name).collect();
    if !skills.is_empty() {
        names.push("read_skill".into());
    }
    if let Some(workspace) = workspace {
        // Listed with commands included: the switch for those is
        // `allow_commands`, and showing the tool explains what that switch does.
        names.extend(workspace::tools(workspace, true).iter().map(|t| t.spec().name));
    }
    names.sort();
    names
}

/// Everything the agent may do, in one place.
pub fn tools(
    store: &Store,
    search: &Arc<dyn SearchIndex>,
    scrape: &Arc<yk_scrape::ScrapeEngine>,
    outside: &Arc<yk_scrape::search::SearchEngine>,
) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(SearchLibrary { store: store.clone(), search: search.clone() }),
        Arc::new(GetItem { store: store.clone() }),
        Arc::new(ReadPaper { store: store.clone() }),
        Arc::new(ListReferences { store: store.clone() }),
        Arc::new(LibraryOverview { store: store.clone() }),
    ];
    tools.extend(ACTIONS.iter().map(|action| {
        Arc::new(LibraryAction {
            action: *action,
            store: store.clone(),
            scrape: scrape.clone(),
            search: outside.clone(),
        }) as Arc<dyn Tool>
    }));
    tools
}

//! Yinkote server: composition root.
//!
//! Wiring order matters and is deliberate:
//! store → search → host bridge → plugins → app state → router.
//! Each layer only knows about the abstractions below it.

pub mod agent;
pub mod badges;
pub mod config;
mod error;
pub mod hostapi;
pub mod integration;
pub mod maintenance;
pub mod routes;
pub mod security;
pub mod storage;
pub mod state;
pub mod naming;
pub mod notes;
pub mod runs;
mod workers;

use std::sync::Arc;
use std::time::Instant;

use axum::http::{header, HeaderValue};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use yk_core::event::EventBus;
use yk_core::ports::{PluginHost, SearchIndex};
use yk_ai::{LocalEmbedder, OpenAiEmbedder};
use yk_search::SearchEngine;
use yk_store::Store;

use config::Config;
use hostapi::HostBridge;
use state::{App, AppState, Services};

/// Build the whole application from configuration.
///
/// Tests use [`build_with_store`] directly so they can supply an in-memory
/// database; both paths share the same wiring below.
pub async fn build(config: Config) -> anyhow::Result<App> {
    let store = Store::open(Some(&config.database_path()))?;
    build_with_store(config, store).await
}

pub async fn build_with_store(config: Config, store: Store) -> anyhow::Result<App> {
    let embedder = make_embedder(&config);
    tracing::info!(provider = embedder.id(), dim = embedder.dimensions(), "embeddings");

    let search: Arc<dyn SearchIndex> = Arc::new(SearchEngine::new(store.clone(), embedder)?);
    let services = Arc::new(Services {
        default_library: store.default_library,
        store,
        search,
        scrape: Arc::new(yk_scrape::ScrapeEngine::with_defaults()),
        storage: Arc::new(storage::Storage::new(config.storage_dir())),
        events: EventBus::default(),
    });

    let disabled: Vec<String> = services
        .store
        .settings
        .get("plugins.disabled")
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let mut builder = yk_plugin::PluginHostBuilder::new().disabled(disabled);
    for dir in config.all_plugin_dirs() {
        std::fs::create_dir_all(&dir).ok();
        builder = builder.dir(dir);
    }
    let plugins: Arc<dyn PluginHost> = builder.build(HostBridge::new(services.clone())).await?;

    let badges = badges::BadgeService::new(plugins.clone());
    let agent = build_agent(&config, &services);
    Ok(Arc::new(AppState {
        services,
        plugins,
        badges,
        config: parking_lot::RwLock::new(config),
        agent: parking_lot::RwLock::new(agent),
        started: Instant::now(),
        harvest: Default::default(),
        runs: Default::default(),
        sessions: Default::default(),
    }))
}

/// The agent, when a model has been named.
///
/// A misconfigured endpoint disables the agent rather than refusing to start:
/// the library is the point of the program, and it must open regardless.
pub fn build_agent(
    config: &Config,
    services: &Arc<state::Services>,
) -> Option<Arc<yk_agent::Agent>> {
    if !config.agent.is_configured() {
        return None;
    }
    // Skills are documents on disk, so a lab can teach the assistant its own
    // procedure without rebuilding anything. The built-ins are written out
    // once and then left alone — an edited skill is the user's.
    let skills_dir = config.skills_dir();
    if let Err(error) = agent::skills::install_builtins(&skills_dir) {
        tracing::warn!(%error, "could not write the built-in skills");
    }
    let skills = Arc::new(yk_agent::skills::Skills::new(
        yk_agent::skills::Skills::load_dir(&skills_dir)
            .iter()
            .filter(|s| config.agent.skill_enabled(&s.name))
            .cloned()
            .collect(),
    ));
    tracing::info!(skills = skills.len(), dir = %skills_dir.display(), "agent skills");

    let workspace = match agent::Workspace::new(config.workspace_dir()) {
        Ok(workspace) => Some(workspace),
        Err(error) => {
            tracing::warn!(%error, "agent workspace unavailable; file tools disabled");
            None
        }
    };

    match agent::provider(&config.agent) {
        Ok(provider) => {
            let mut tools = agent::tools(&services.store, &services.search, &services.scrape);
            if !skills.is_empty() {
                tools.push(Arc::new(yk_agent::skills::ReadSkill { skills: skills.clone() }));
            }
            if let Some(workspace) = &workspace {
                tools.extend(agent::workspace::tools(workspace, config.agent.allow_commands));
            }
            // Filtered once, here, rather than checked at every call site:
            // a tool the assistant was never given cannot be forgotten about.
            tools.retain(|t| config.agent.tool_enabled(&t.spec().name));
            let system = format!("{}{}", agent::SYSTEM_PROMPT, skills.prompt_section());
            let agent = yk_agent::Agent::new(Arc::new(provider), tools, system)
                .with_max_steps(config.agent.max_steps);
            Some(Arc::new(agent))
        }
        Err(error) => {
            tracing::warn!(%error, "agent disabled");
            None
        }
    }
}

fn make_embedder(config: &Config) -> Arc<dyn yk_ai::EmbeddingProvider> {
    let e = &config.embeddings;
    match (e.provider.as_str(), &e.endpoint, &e.model) {
        ("remote", Some(endpoint), Some(model)) => Arc::new(OpenAiEmbedder::new(
            endpoint,
            model,
            e.api_key.clone(),
            e.dimensions,
        )),
        ("remote", ..) => {
            tracing::warn!("remote embeddings need endpoint and model; falling back to local");
            Arc::new(LocalEmbedder::new())
        }
        _ => Arc::new(LocalEmbedder::new()),
    }
}

/// Assemble the HTTP router: API under `/api/v1`, workbench everywhere else.
pub fn router(app: App) -> Router {
    let api = routes::router()
        .layer(axum::middleware::from_fn_with_state(app.clone(), security::guard));

    // The connector sits outside the API guard and outside `/api/v1`: the
    // browser extension can hold no key and knows only Zotero's paths. It is
    // reachable only on loopback, and accepts only the shapes it defines.
    let mut router = Router::new().nest("/api/v1", api).merge(routes::connector::router());

    let started_config = app.config();
    if let Some(dir) = &started_config.web_dir {
        let index = dir.join("index.html");
        if index.exists() {
            // SPA fallback: unknown paths render the app shell, not a 404.
            router = router.fallback_service(
                ServeDir::new(dir).fallback(ServeFile::new(index)),
            );
            tracing::info!(dir = %dir.display(), "serving workbench");
        } else {
            tracing::warn!(dir = %dir.display(), "web_dir has no index.html; UI disabled");
        }
    }

    router
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}

/// Bind and serve until the process is asked to stop.
pub async fn serve(app: App) -> anyhow::Result<()> {
    let addr = app.config().bind_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "yinkote listening — open http://{local}");

    workers::spawn(app.clone());
    serve_connector(&app).await;

    let plugins = app.plugins.clone();
    axum::serve(listener, router(app))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutting down plugins");
    plugins.shutdown().await;
    Ok(())
}

/// Listen on the port the browser extension knows, if asked to.
///
/// Never by default. That port is Zotero's, and taking it from a running Zotero
/// would break the very thing a user is in the middle of migrating away from —
/// the one failure they would least forgive. Asking for it is a statement that
/// they are not running both.
///
/// A refusal to bind is reported and survived: the workbench is the product,
/// and it must not fail to start because a port is busy.
async fn serve_connector(app: &App) {
    let Some(port) = app.config().connector_port else { return };
    let addr = format!("127.0.0.1:{port}");

    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            tracing::info!(%addr, "browser connector listening");
            let router = router(app.clone());
            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    tracing::warn!(error = %e, "browser connector stopped");
                }
            });
        }
        Err(e) => tracing::warn!(
            %addr,
            error = %e,
            "could not listen for the browser connector; is Zotero running?"
        ),
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

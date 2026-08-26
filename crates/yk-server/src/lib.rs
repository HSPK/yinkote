//! Yinkote server: composition root.
//!
//! Wiring order matters and is deliberate:
//! store → search → host bridge → plugins → app state → router.
//! Each layer only knows about the abstractions below it.

pub mod config;
pub mod error;
pub mod hostapi;
pub mod routes;
pub mod security;
pub mod state;
pub mod workers;

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
use yk_search::{LocalEmbedder, RemoteEmbedder, SearchEngine};
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

    Ok(Arc::new(AppState { services, plugins, config, started: Instant::now() }))
}

fn make_embedder(config: &Config) -> Arc<dyn yk_core::ports::EmbeddingProvider> {
    let e = &config.embeddings;
    match (e.provider.as_str(), &e.endpoint, &e.model) {
        ("remote", Some(endpoint), Some(model)) => Arc::new(RemoteEmbedder::new(
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

    let mut router = Router::new().nest("/api/v1", api);

    if let Some(dir) = &app.config.web_dir {
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
    let addr = app.config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "yinkote listening — open http://{local}");

    workers::spawn(app.clone());

    let plugins = app.plugins.clone();
    axum::serve(listener, router(app))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutting down plugins");
    plugins.shutdown().await;
    Ok(())
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

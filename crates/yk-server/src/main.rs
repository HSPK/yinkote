//! `yinkote` — the background service that hosts the library, the API and the
//! web workbench.

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;
use yk_server::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.help {
        println!("{USAGE}");
        return Ok(());
    }
    if args.version {
        println!("yinkote {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("YK_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=warn")),
        )
        .with_target(false)
        .init();

    let mut config = Config::load(args.data_dir);
    if let Some(p) = args.connector_port {
        config.connector_port = Some(p);
    }
    if let Some(p) = args.port {
        config.port = p;
    }
    if let Some(h) = args.host {
        config.host = h;
    }
    if let Some(w) = args.web_dir {
        config.web_dir = Some(w);
    }
    config.plugin_dirs.extend(args.plugin_dirs);
    // Convenience: pick up a sibling `web/dist` during development.
    if config.web_dir.is_none() {
        for candidate in ["web/dist", "../web/dist", "../../web/dist"] {
            let p = PathBuf::from(candidate);
            if p.join("index.html").exists() {
                config.web_dir = Some(p);
                break;
            }
        }
    }

    std::fs::create_dir_all(config.data_dir())?;
    tracing::info!(data_dir = %config.data_dir().display(), "starting yinkote");

    let app = yk_server::build(config).await?;
    yk_server::serve(app).await
}

const USAGE: &str = "\
yinkote — local-first reference manager

USAGE:
    yinkote [OPTIONS]

OPTIONS:
    -p, --port <PORT>        Port to listen on (default 23130)
        --host <HOST>        Address to bind (default 127.0.0.1)
        --data-dir <DIR>     Data directory (default: platform data dir)
        --web-dir <DIR>      Directory containing the built workbench
        --plugin-dir <DIR>   Extra plugin directory (repeatable)
        --connector-port <PORT>
                             Also listen for the Zotero browser extension,
                             conventionally 23119. Off unless asked for: that
                             port belongs to Zotero, and taking it would break
                             a running copy.
    -h, --help               Print this help
    -V, --version            Print version

ENVIRONMENT:
    YK_DATA_DIR, YK_HOST, YK_PORT, YK_WEB_DIR, YK_API_KEY, YK_LOG
    YK_EMBED_ENDPOINT, YK_EMBED_MODEL, YK_EMBED_API_KEY, YK_EMBED_DIM\n    YK_AGENT_ENDPOINT, YK_AGENT_MODEL, YK_AGENT_API_KEY
";

/// Hand-rolled to keep the dependency tree small; the surface is tiny.
#[derive(Default)]
struct Args {
    port: Option<u16>,
    connector_port: Option<u16>,
    host: Option<String>,
    data_dir: Option<PathBuf>,
    web_dir: Option<PathBuf>,
    plugin_dirs: Vec<PathBuf>,
    help: bool,
    version: bool,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args::default();
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "-h" | "--help" => args.help = true,
                "-V" | "--version" => args.version = true,
                "-p" | "--port" => args.port = it.next().and_then(|v| v.parse().ok()),
                "--host" => args.host = it.next(),
                "--data-dir" => args.data_dir = it.next().map(PathBuf::from),
                "--web-dir" => args.web_dir = it.next().map(PathBuf::from),
                "--plugin-dir" => args.plugin_dirs.extend(it.next().map(PathBuf::from)),
                "--connector-port" => {
                    args.connector_port = it.next().and_then(|v| v.parse().ok())
                }
                other => eprintln!("warning: ignoring unknown argument '{other}'"),
            }
        }
        args
    }
}

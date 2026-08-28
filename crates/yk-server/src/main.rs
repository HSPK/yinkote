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

    let mut config = Config::load(args.data_dir.clone());
    if let Some(p) = args.port {
        config.port = p;
    }

    // Before anything is started: these manage the service rather than being
    // it, and starting a second copy to install an autostart file would be an
    // odd thing to do.
    if let Some(command) = &args.service {
        return run_service(command, &config);
    }
    if let Some(p) = args.connector_port {
        config.connector_port = Some(p);
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

    // One server per data directory. Two sharing one library do not fail —
    // they quietly disagree, each with its own copy of the search index — so
    // the check has to happen here rather than being noticed later.
    //
    // Bound to a name that lives as long as `main`: an advisory lock is
    // released when the file is dropped, and `let _ = …` would drop it on this
    // very line, leaving the directory unlocked and the check pointless.
    let _claim = match yk_server::lock::acquire(&config.data_dir(), config.port) {
        Ok(lock) => lock,
        Err(denied) => {
            eprintln!("{denied}");
            std::process::exit(1);
        }
    };

    tracing::info!(data_dir = %config.data_dir().display(), "starting yinkote");

    let app = yk_server::build(config).await?;
    yk_server::serve(app).await
}

/// `yinkote service …` — arranging for the program to start at login.
///
/// Kept out of `main` because it is a different program: it writes one file and
/// exits, and every line of it is about reporting exactly what happened. An
/// install that says "done" while leaving a unit systemd has not been told
/// about is worse than one that says what is left to do.
fn run_service(command: &str, config: &Config) -> anyhow::Result<()> {
    use yk_server::service;
    match command {
        "install" => {
            let done = service::install(&config.data_dir(), config.port)?;
            println!("wrote {}", done.path.display());
            match done.activation {
                Some(next) => println!("\nnow run:\n    {next}"),
                None => println!("it will start at your next login"),
            }
        }
        "uninstall" => match service::uninstall()? {
            Some(path) => println!("removed {}", path.display()),
            None => println!("nothing was installed"),
        },
        "status" => match service::status() {
            Some(path) => println!("installed: {}", path.display()),
            None => println!("not installed"),
        },
        other => {
            anyhow::bail!("unknown service command '{other}'; expected install, uninstall or status")
        }
    }
    Ok(())
}

const USAGE: &str = "\
yinkote — local-first reference manager

USAGE:
    yinkote [OPTIONS]
    yinkote service install|uninstall|status

    `service install` writes an autostart file for the current user — a
    systemd user unit, a launchd agent, or a Startup-folder script — using
    the --data-dir and --port given alongside it. Never a system service:
    a personal library does not belong to root.

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
    /// `install`, `uninstall` or `status`; `None` means "be the server".
    service: Option<String>,
    help: bool,
    version: bool,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args::default();
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "service" => args.service = Some(it.next().unwrap_or_else(|| "status".into())),
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

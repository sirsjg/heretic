//! Heretic without the window.
//!
//! Runs the engine and the remote server on their own, for a machine that has
//! the repositories and the agent CLIs but no screen: a home server, a box
//! under the desk. The interface is then reached from a browser anywhere.
//!
//! It shares the settings file and the run history with the desktop app, so
//! run one or the other on a machine — both at once would each pick up the
//! same ready tasks.

use heretic_core::config::RemoteConfig;
use heretic_core::Service;
use std::sync::Arc;

const USAGE: &str = "\
heretic-serve — run Heretic headless and serve its interface over HTTP

USAGE:
    heretic-serve [--bind ADDR] [--port PORT] [--public-url URL]

OPTIONS:
    --bind ADDR         Interface to listen on (default: the saved setting,
                        or 127.0.0.1). Use 0.0.0.0 for every interface, or a
                        Tailscale address for just that one.
    --port PORT         Port to listen on (default: the saved setting, or 7411).
    --public-url URL    The address clients reach this at, when it is behind
                        Tailscale Serve or a reverse proxy.
    -h, --help          Show this.

The bearer token is the one in settings; one is minted and saved on first use.
The pairing link is printed on start. Settings live in the same place the
desktop app keeps them — configure the Flux server, the models and the project
folders there first, or edit settings.json directly.";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "heretic_serve=info,heretic_server=info,heretic_core=info".into()
            }),
        )
        .init();

    let overrides = match parse(std::env::args().skip(1)) {
        Ok(Some(overrides)) => overrides,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let runtime = tokio::runtime::Runtime::new().expect("a Tokio runtime");
    if let Err(message) = runtime.block_on(serve(overrides)) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[derive(Default)]
struct Overrides {
    bind: Option<String>,
    port: Option<u16>,
    public_url: Option<String>,
}

fn parse(args: impl Iterator<Item = String>) -> Result<Option<Overrides>, String> {
    let mut overrides = Overrides::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--bind" => overrides.bind = Some(args.next().ok_or("--bind needs an address")?),
            "--port" => {
                let value = args.next().ok_or("--port needs a number")?;
                overrides.port = Some(
                    value
                        .parse()
                        .map_err(|_| format!("{value} is not a port number"))?,
                );
            }
            "--public-url" => {
                overrides.public_url = Some(args.next().ok_or("--public-url needs an address")?)
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Some(overrides))
}

async fn serve(overrides: Overrides) -> Result<(), String> {
    let service = Arc::new(Service::load());

    // The daemon exists to serve, so the settings' on/off switch does not
    // apply here — but the rest of the remote configuration does, and what
    // is minted here is saved so the desktop app agrees with it.
    let mut settings = service.settings().await;
    let mut remote = RemoteConfig {
        enabled: true,
        ..settings.remote.clone()
    };
    if let Some(bind) = overrides.bind {
        remote.bind = bind;
    }
    if let Some(port) = overrides.port {
        remote.port = port;
    }
    if let Some(url) = overrides.public_url {
        remote.public_url = Some(url);
    }
    if remote.token.as_deref().unwrap_or("").is_empty() {
        remote.token = Some(RemoteConfig::generate_token());
    }
    if settings.remote.token != remote.token {
        settings.remote.token = remote.token.clone();
        service.save_settings(settings).await?;
    }

    let running = heretic_server::start(Arc::clone(&service), &remote)
        .await
        .map_err(|error| error.to_string())?;

    let token = remote.token.clone().unwrap_or_default();
    println!("Heretic is listening on {}", running.addr);
    let candidates: Vec<String> = if running.addr.ip().is_unspecified() {
        heretic_server::net::interfaces()
            .into_iter()
            .map(|iface| remote.base_url_for(&iface.ip.to_string()))
            .collect()
    } else {
        vec![remote.base_url_for(&running.addr.ip().to_string())]
    };
    for base in candidates {
        println!("  open {base}/#token={token}");
    }
    println!("Press Ctrl-C to stop.");

    let background = tokio::spawn(Arc::clone(&service).run_background());
    let _ = tokio::signal::ctrl_c().await;
    background.abort();
    running.stop().await;
    Ok(())
}

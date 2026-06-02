//! gproxy v2 binary: wire config + state + router and serve.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use gproxy::app::AppState;
use gproxy::config::{self, SharedConfig};
use gproxy::http;
use gproxy::store::cache::MemoryCache;

#[derive(Parser, Debug)]
#[command(name = "gproxy", version, about = "gproxy v2 LLM proxy")]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long, env = "GPROXY_CONFIG", default_value = "./gproxy.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let runtime_config = config::load_config(&cli.config)?;
    let bind = runtime_config.bind_addr()?;
    let shared: SharedConfig = config::shared(runtime_config);

    let state = AppState::new(shared, Arc::new(MemoryCache::new()));
    let app = http::router(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("gproxy v2 listening on http://{bind}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => tracing::warn!("failed to install SIGTERM handler: {e}"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

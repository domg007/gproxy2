//! gproxy v2 binary: parse CLI/env config, wire persistence + state + router, serve.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use gproxy::app::AppState;
use gproxy::config::{self, PersistenceKind, RuntimeConfig, SharedConfig};
use gproxy::http;
use gproxy::store::cache::MemoryCache;
use gproxy::store::persistence::{DbPersistence, FilePersistence, PersistenceBackend};

#[derive(Parser, Debug)]
#[command(name = "gproxy", version, about = "gproxy v2 LLM proxy")]
struct Cli {
    /// Bind host (IPv6 must use bracket notation, e.g. [::1]).
    #[arg(long, env = "GPROXY_HOST", default_value = "127.0.0.1")]
    host: String,

    /// Bind port.
    #[arg(long, env = "GPROXY_PORT", default_value_t = 8787)]
    port: u16,

    /// Persistence backend: `file` (local disk) or `db` (SeaORM).
    #[arg(long, env = "GPROXY_PERSISTENCE", default_value = "file")]
    persistence: PersistenceKind,

    /// Data directory used by the file persistence backend.
    #[arg(long, env = "GPROXY_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    /// Database connection string (required when --persistence=db).
    #[arg(long, env = "GPROXY_DSN")]
    dsn: Option<String>,

    /// Logical name for this instance (used in diagnostics / multi-node setups).
    #[arg(long, env = "GPROXY_INSTANCE_NAME", default_value = "default")]
    instance_name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let runtime_config = RuntimeConfig {
        host: cli.host,
        port: cli.port,
        persistence: cli.persistence,
        data_dir: cli.data_dir,
        dsn: cli.dsn,
        instance_name: cli.instance_name,
    };
    runtime_config.validate()?;

    let bind = runtime_config.bind_addr()?;
    let shared: SharedConfig = config::shared(runtime_config.clone());

    let persistence: Arc<dyn PersistenceBackend> = match runtime_config.persistence {
        PersistenceKind::File => {
            Arc::new(FilePersistence::open(runtime_config.data_dir.clone()).await?)
        }
        PersistenceKind::Db => {
            Arc::new(DbPersistence::connect(runtime_config.dsn.as_deref().unwrap()).await?)
        }
    };

    persistence.health().await?;
    tracing::info!("persistence backend: {} healthy", persistence.kind());

    let state = AppState::new(shared, Arc::new(MemoryCache::new()), persistence);
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

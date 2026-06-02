//! gproxy v2 binary: parse CLI/env config, wire persistence + state + router, serve.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use gproxy::app::AppState;
use gproxy::config::{CacheConfig, PersistenceConfig, PersistenceKind, RuntimeConfig};
use gproxy::http;
use gproxy::store::cache::{CacheBackend, MemoryCache, RedisCache};
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

    /// Redis URL for the shared cache backend (e.g. redis://127.0.0.1:6379).
    /// Omit to use the in-process memory cache.
    #[arg(long, env = "GPROXY_REDIS_URL")]
    redis_url: Option<String>,

    /// Numeric identifier for this instance (used to partition per-instance
    /// rows in the database; set distinct values across a multi-node fleet).
    #[arg(long, env = "GPROXY_INSTANCE_ID", default_value_t = 0)]
    instance_id: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let cache_cfg = CacheConfig::from_url(cli.redis_url);
    let persistence_cfg = PersistenceConfig::from_parts(cli.persistence, cli.data_dir, cli.dsn)?;

    let config = Arc::new(RuntimeConfig {
        host: cli.host,
        port: cli.port,
        cache: cache_cfg,
        persistence: persistence_cfg,
        instance_id: cli.instance_id,
    });

    let bind = config.bind_addr()?;

    let cache: Arc<dyn CacheBackend> = match &config.cache {
        CacheConfig::Memory => {
            tracing::info!("cache backend: memory ready");
            Arc::new(MemoryCache::new())
        }
        CacheConfig::Redis { url } => {
            let c = RedisCache::connect(url).await?;
            c.health().await?;
            tracing::info!("cache backend: redis ready");
            Arc::new(c)
        }
    };

    let persistence: Arc<dyn PersistenceBackend> = match &config.persistence {
        PersistenceConfig::File { data_dir } => {
            Arc::new(FilePersistence::open(data_dir.clone()).await?)
        }
        PersistenceConfig::Db { dsn } => Arc::new(DbPersistence::connect(dsn).await?),
    };
    persistence.health().await?;
    tracing::info!("persistence backend: {} healthy", persistence.kind());

    let state = AppState::new(config, cache, persistence);
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

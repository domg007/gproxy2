//! gproxy v2 binary: parse CLI/env config, wire persistence + state + router, serve.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use gproxy::app::AppState;
use gproxy::config::{
    CacheConfig, PersistenceConfig, PersistenceKind, RuntimeConfig, UpstreamConfig,
};
use gproxy::http;
use gproxy::http::client::UpstreamClient;
use gproxy::store::cache::CacheBackend;
use gproxy::store::persistence::PersistenceBackend;

#[derive(Parser)]
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

    /// Optional native proxy URL for upstream provider requests.
    #[arg(long, env = "GPROXY_UPSTREAM_PROXY_URL")]
    upstream_proxy_url: Option<String>,

    /// Numeric identifier for this instance (used to partition per-instance
    /// rows in the database; set distinct values across a multi-node fleet).
    #[arg(long, env = "GPROXY_INSTANCE_ID", default_value_t = 0)]
    instance_id: u64,

    /// Per-request failover attempt cap: the loop stops after this many
    /// candidate attempts even if more remain (bounds fan-out on a large
    /// unhealthy pool). The AuthDead forced-refresh retry does not count.
    #[arg(long, env = "GPROXY_MAX_ATTEMPTS", default_value_t = gproxy::config::DEFAULT_MAX_ATTEMPTS)]
    max_attempts: u32,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Import a config bundle (JSON) into the persistence backend, then exit.
    Import {
        /// Path to the bundle file.
        #[arg(long = "in")]
        input: PathBuf,
    },
    /// Export all control-plane config (with PLAINTEXT secrets) to a bundle
    /// file that `import` consumes, then exit.
    Export {
        /// Path to write the bundle file.
        #[arg(long = "out")]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let cache_cfg = CacheConfig::from_url(cli.redis_url);
    let persistence_cfg = PersistenceConfig::from_parts(cli.persistence, cli.data_dir, cli.dsn)?;
    let upstream_cfg = UpstreamConfig::from_proxy_url(cli.upstream_proxy_url);

    let config = Arc::new(RuntimeConfig {
        host: cli.host,
        port: cli.port,
        cache: cache_cfg,
        persistence: persistence_cfg,
        upstream: upstream_cfg,
        instance_id: cli.instance_id,
        max_attempts: cli.max_attempts,
    });

    let bind = config.bind_addr()?;

    // Persistence is built first — the import subcommand and first-boot hook
    // both need it before the (optional) cache backend is started.
    let persistence: Arc<dyn PersistenceBackend> = match &config.persistence {
        #[cfg(feature = "persist-file")]
        PersistenceConfig::File { data_dir } => {
            Arc::new(gproxy::store::persistence::FilePersistence::open(data_dir.clone()).await?)
        }
        #[cfg(not(feature = "persist-file"))]
        PersistenceConfig::File { .. } => {
            anyhow::bail!("persistence backend `file` requires the `persist-file` feature")
        }
        #[cfg(feature = "persist-db")]
        PersistenceConfig::Db { dsn } => {
            Arc::new(gproxy::store::persistence::DbPersistence::connect(dsn).await?)
        }
        #[cfg(not(feature = "persist-db"))]
        PersistenceConfig::Db { .. } => {
            anyhow::bail!("persistence backend `db` requires the `persist-db` feature")
        }
    };
    persistence.health().await?;
    tracing::info!("persistence backend: {} healthy", persistence.kind());

    // Envelope cipher (§14.1): GPROXY_MASTER_KEY is env-only (§8-E — never a
    // CLI flag). Malformed key = hard boot error; absent key = plaintext mode.
    let master_key = std::env::var("GPROXY_MASTER_KEY").ok();
    if master_key.is_none() {
        tracing::warn!("GPROXY_MASTER_KEY not set; secrets stored and read as plaintext");
    }
    let cipher = gproxy::crypto::cipher_from_master_key(master_key.as_deref())?;

    // Config subcommands: import / export, then exit — no server started.
    match cli.command {
        Some(Command::Import { input }) => {
            let json = std::fs::read_to_string(&input)?;
            let stats =
                gproxy::app::import::import_bundle(persistence.as_ref(), cipher.as_ref(), &json)
                    .await?;
            tracing::info!(records = stats.records, "bundle imported");
            return Ok(());
        }
        Some(Command::Export { output }) => {
            let bundle =
                gproxy::app::export::export_bundle(persistence.as_ref(), cipher.as_ref()).await?;
            let json = serde_json::to_string_pretty(&bundle)?;
            std::fs::write(&output, json)?;
            tracing::warn!(
                "exported config to {output:?} — contains PLAINTEXT secrets; chmod 600 and protect this file"
            );
            return Ok(());
        }
        None => {}
    }

    // First-boot hook: if GPROXY_IMPORT_FILE is set and the store is empty,
    // seed it from the bundle before building the snapshot.
    if let Ok(path) = std::env::var("GPROXY_IMPORT_FILE")
        && !path.is_empty()
    {
        let empty = persistence.list_providers().await?.is_empty()
            && persistence.list_users().await?.is_empty();
        if empty {
            let json = std::fs::read_to_string(&path)?;
            let stats =
                gproxy::app::import::import_bundle(persistence.as_ref(), cipher.as_ref(), &json)
                    .await?;
            tracing::info!(records = stats.records, path, "first-boot bundle imported");
        } else {
            tracing::info!(path, "GPROXY_IMPORT_FILE set but store not empty; skipped");
        }
    }

    let cache: Arc<dyn CacheBackend> = match &config.cache {
        #[cfg(feature = "cache-memory")]
        CacheConfig::Memory => {
            tracing::info!("cache backend: memory ready");
            Arc::new(gproxy::store::cache::MemoryCache::new())
        }
        #[cfg(not(feature = "cache-memory"))]
        CacheConfig::Memory => {
            anyhow::bail!("cache backend `memory` requires the `cache-memory` feature")
        }
        #[cfg(feature = "cache-redis")]
        CacheConfig::Redis { url } => {
            let c = gproxy::store::cache::RedisCache::connect(url).await?;
            c.health().await?;
            tracing::info!("cache backend: redis ready");
            Arc::new(c)
        }
        #[cfg(not(feature = "cache-redis"))]
        CacheConfig::Redis { .. } => {
            anyhow::bail!("cache backend `redis` requires the `cache-redis` feature")
        }
        CacheConfig::Libsql { .. } | CacheConfig::Upstash { .. } => {
            anyhow::bail!("edge-only cache backend cannot be used by native server")
        }
    };

    #[cfg(not(feature = "upstream-wreq"))]
    compile_error!("a native gproxy binary requires the `upstream-wreq` feature");
    #[cfg(feature = "upstream-wreq")]
    let upstream: Arc<dyn UpstreamClient> = Arc::new(
        gproxy::http::client::WreqClient::with_proxy_url(config.upstream.proxy_url.as_deref())?,
    );
    #[cfg(feature = "upstream-wreq")]
    tracing::info!(
        "upstream transport: wreq ready{}",
        if config.upstream.proxy_url.is_some() {
            " with proxy"
        } else {
            ""
        }
    );

    let snapshot =
        gproxy::app::snapshot::ControlPlaneSnapshot::build(persistence.as_ref(), 1).await?;
    let snapshot = Arc::new(arc_swap::ArcSwap::from_pointee(snapshot));
    let channels = Arc::new(gproxy::channel::registry::ChannelRegistry::with_builtin());

    let state = AppState::new(
        config,
        cache,
        persistence,
        upstream,
        snapshot,
        channels,
        cipher,
    );

    // Tokenizer registry (§6.3): vocab storage rides the persistence backend
    // (file = raw files under data_dir/tokenizers, db = BLOB rows); only the
    // download toggle is seeded here from instance settings.
    #[cfg(feature = "count-local")]
    {
        let enabled = state
            .persistence
            .list_instance_settings()
            .await?
            .first()
            .is_some_and(|s| s.enable_tokenizer_download);
        state.tokenizers.set_download_enabled(enabled);
    }

    // Multi-instance: listen for cross-instance config invalidation (redis only;
    // memory cache is single-instance and its subscribe is a no-op).
    if matches!(state.config.cache, CacheConfig::Redis { .. }) {
        gproxy::app::invalidation::spawn_invalidation_listener(state.clone());
    }

    let app = http::server::router(state);

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

use std::path::Path;
use std::sync::Arc;

use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use gproxy_sdk::provider::engine::GproxyEngineBuilder;
use gproxy_server::{AppStateBuilder, GlobalConfig};
use gproxy_storage::{SeaOrmStorage, StorageWriteWorkerConfig};

#[derive(Parser)]
#[command(name = "gproxy", about = "High-performance LLM proxy server")]
struct Cli {
    /// Listen host
    #[arg(long, env = "GPROXY_HOST", default_value = "127.0.0.1")]
    host: String,

    /// Listen port
    #[arg(long, env = "GPROXY_PORT", default_value_t = 8787)]
    port: u16,

    /// Admin API key (generated randomly if not set)
    #[arg(long, env = "GPROXY_ADMIN_KEY")]
    admin_key: Option<String>,

    /// Database connection string (default: sqlite in data_dir)
    #[arg(long, env = "GPROXY_DSN")]
    dsn: Option<String>,

    /// Path to TOML config file for initial seeding
    #[arg(long, env = "GPROXY_CONFIG", default_value = "gproxy.toml")]
    config: String,

    /// Data directory
    #[arg(long, env = "GPROXY_DATA_DIR", default_value = "./data")]
    data_dir: String,

    /// HTTP proxy for upstream requests
    #[arg(long, env = "GPROXY_PROXY")]
    proxy: Option<String>,

    /// TLS fingerprint emulation
    #[arg(long, env = "GPROXY_SPOOF", default_value = "chrome_136")]
    spoof_emulation: String,

    /// Database encryption secret key (XChaCha20Poly1305).
    /// When set, credentials, passwords, and API keys are encrypted at rest.
    #[arg(long, env = "DATABASE_SECRET_KEY")]
    database_secret_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // 2. Parse CLI
    let cli = Cli::parse();

    // 3. Resolve DSN
    let dsn = cli.dsn.clone().unwrap_or_else(|| {
        let db_path = Path::new(&cli.data_dir).join("gproxy.db");
        format!("sqlite://{}?mode=rwc", db_path.display())
    });

    // 4. Ensure data directory exists
    std::fs::create_dir_all(&cli.data_dir)?;

    // 5. Connect database + sync schema
    tracing::info!(dsn = %dsn, "connecting to database");
    let storage = SeaOrmStorage::connect(&dsn, cli.database_secret_key.as_deref()).await?;
    storage.sync().await?;
    let storage = Arc::new(storage);
    tracing::info!("database schema synced");

    // 6. Storage write channel + worker
    let (write_tx, write_rx) = gproxy_storage::storage_write_channel(1024);
    let _write_worker = gproxy_storage::spawn_storage_write_worker(
        storage.clone(),
        write_rx,
        StorageWriteWorkerConfig::default(),
    );

    // 7. Build initial GlobalConfig from CLI
    let admin_key = cli.admin_key.unwrap_or_else(|| {
        let key = uuid::Uuid::now_v7().to_string();
        tracing::info!(admin_key = %key, "generated admin key (save this!)");
        key
    });

    let config = GlobalConfig {
        host: cli.host.clone(),
        port: cli.port,
        admin_key,
        proxy: cli.proxy.clone(),
        spoof_emulation: cli.spoof_emulation.clone(),
        update_source: "github".to_string(),
        enable_usage: true,
        enable_upstream_log: true,
        enable_upstream_log_body: true,
        enable_downstream_log: true,
        enable_downstream_log_body: true,
        dsn: dsn.clone(),
        data_dir: cli.data_dir.clone(),
    };

    // 8. Build empty engine + AppState
    let engine = GproxyEngineBuilder::new()
        .configure_clients(config.proxy.as_deref(), Some(&config.spoof_emulation))
        .build();

    let state = Arc::new(
        AppStateBuilder::new()
            .engine(engine)
            .storage(storage.clone())
            .storage_writes(write_tx)
            .config(config)
            .build(),
    );

    // 9. Bootstrap: load from DB or seed from TOML / defaults
    let has_data = storage.get_global_settings().await?.is_some();

    if has_data {
        tracing::info!("loading state from database");
        let counts = gproxy_api::bootstrap::reload_from_db(&state)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        tracing::info!(
            providers = counts.providers,
            users = counts.users,
            keys = counts.keys,
            models = counts.models,
            "bootstrap from database complete"
        );
    } else {
        let toml_path = Path::new(&cli.config);
        if toml_path.exists() {
            tracing::info!(path = %toml_path.display(), "seeding from TOML config");
            let toml_str = std::fs::read_to_string(toml_path)?;
            gproxy_api::bootstrap::seed_from_toml(&state, &toml_str)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            tracing::info!("TOML seed complete, data persisted to database");
        } else {
            tracing::info!("no existing data or config file, creating defaults");
            gproxy_api::bootstrap::seed_defaults(&state)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }

    // 10. Build router and start server
    let app = gproxy_api::api_router(state);
    let bind_addr = format!("{}:{}", cli.host, cli.port);
    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!(addr = %bind_addr, "gproxy listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("gproxy shut down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("received ctrl+c"),
        () = terminate => tracing::info!("received SIGTERM"),
    }
}

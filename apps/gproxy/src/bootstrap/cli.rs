use std::path::PathBuf;

use clap::Parser;

use crate::bootstrap::config::DEFAULT_CONFIG_PATH;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "gproxy",
    version,
    about = "High-performance multi-provider LLM proxy"
)]
pub struct CliArgs {
    /// Bootstrap config file.
    #[arg(long, env = "GPROXY_CONFIG_PATH", default_value = DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    #[arg(long, env = "GPROXY_HOST")]
    pub host: Option<String>,

    #[arg(long, env = "GPROXY_PORT")]
    pub port: Option<u16>,

    #[arg(long, env = "GPROXY_PROXY")]
    pub proxy: Option<String>,

    #[arg(long, env = "GPROXY_ADMIN_KEY")]
    pub admin_key: Option<String>,

    #[arg(long, env = "GPROXY_MASK_SENSITIVE_INFO")]
    pub mask_sensitive_info: Option<bool>,

    #[arg(long, env = "GPROXY_DATA_DIR")]
    pub data_dir: Option<String>,

    #[arg(long, env = "GPROXY_DSN")]
    pub dsn: Option<String>,

    #[arg(long, env = "GPROXY_STORAGE_WRITE_QUEUE_CAPACITY")]
    pub storage_write_queue_capacity: Option<usize>,

    #[arg(long, env = "GPROXY_STORAGE_WRITE_MAX_BATCH_SIZE")]
    pub storage_write_max_batch_size: Option<usize>,

    #[arg(long, env = "GPROXY_STORAGE_WRITE_AGGREGATE_WINDOW_MS")]
    pub storage_write_aggregate_window_ms: Option<u64>,
}

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::channel::StorageWriteReceiver;
use super::event::StorageWriteBatch;

#[derive(Debug, Clone)]
pub struct StorageWriteWorkerConfig {
    pub max_batch_size: usize,
    pub aggregate_window: Duration,
}

impl Default for StorageWriteWorkerConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1024,
            aggregate_window: Duration::from_millis(25),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct StorageWriteSinkError {
    pub message: String,
}

impl StorageWriteSinkError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub trait StorageWriteSink: Send + Sync + 'static {
    fn write_batch<'a>(
        &'a self,
        batch: StorageWriteBatch,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageWriteSinkError>> + Send + 'a>>;
}

pub async fn run_storage_write_worker<S: StorageWriteSink>(
    sink: Arc<S>,
    mut receiver: StorageWriteReceiver,
    config: StorageWriteWorkerConfig,
) -> Result<(), StorageWriteSinkError> {
    while let Some(first) = receiver.recv().await {
        let mut batch = StorageWriteBatch::default();
        batch.apply(first);

        let deadline = tokio::time::Instant::now() + config.aggregate_window;
        while batch.event_count < config.max_batch_size {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            let wait = deadline - now;
            match tokio::time::timeout(wait, receiver.recv()).await {
                Ok(Some(event)) => batch.apply(event),
                Ok(None) => {
                    if !batch.is_empty() {
                        sink.write_batch(batch).await?;
                    }
                    return Ok(());
                }
                Err(_) => break,
            }
        }

        sink.write_batch(batch).await?;
    }
    Ok(())
}

pub fn spawn_storage_write_worker<S: StorageWriteSink>(
    sink: Arc<S>,
    receiver: StorageWriteReceiver,
    config: StorageWriteWorkerConfig,
) -> tokio::task::JoinHandle<Result<(), StorageWriteSinkError>> {
    tokio::spawn(run_storage_write_worker(sink, receiver, config))
}

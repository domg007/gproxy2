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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;
    use crate::write::{
        CredentialStatusWrite, ProviderWrite, StorageWriteEvent, UserWrite, storage_write_channel,
    };

    struct MemorySink {
        batches: Mutex<Vec<StorageWriteBatch>>,
    }

    impl MemorySink {
        fn new() -> Self {
            Self {
                batches: Mutex::new(Vec::new()),
            }
        }
    }

    impl StorageWriteSink for MemorySink {
        fn write_batch<'a>(
            &'a self,
            batch: StorageWriteBatch,
        ) -> Pin<Box<dyn Future<Output = Result<(), StorageWriteSinkError>> + Send + 'a>> {
            Box::pin(async move {
                self.batches
                    .lock()
                    .map_err(|_| StorageWriteSinkError::new("memory sink lock poisoned"))?
                    .push(batch);
                Ok(())
            })
        }
    }

    fn next_test_id() -> i64 {
        static NEXT_ID: AtomicI64 = AtomicI64::new(1);
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queue_aggregates_same_key_before_flush() {
        let (tx, rx) = storage_write_channel(32);
        let sink = Arc::new(MemorySink::new());
        let handle = spawn_storage_write_worker(
            sink.clone(),
            rx,
            StorageWriteWorkerConfig {
                max_batch_size: 128,
                aggregate_window: Duration::from_millis(50),
            },
        );

        let user_id = next_test_id();
        tx.enqueue(StorageWriteEvent::UpsertUser(UserWrite {
            id: user_id,
            name: "alice".to_string(),
            password: "p1".to_string(),
            enabled: true,
        }))
        .await
        .expect("enqueue upsert user 1");
        tx.enqueue(StorageWriteEvent::UpsertUser(UserWrite {
            id: user_id,
            name: "alice-renamed".to_string(),
            password: "p2".to_string(),
            enabled: true,
        }))
        .await
        .expect("enqueue upsert user 2");
        drop(tx);

        handle.await.expect("worker join").expect("worker run");
        let mut batches = sink.batches.lock().expect("batch lock");
        assert_eq!(batches.len(), 1);
        let batch = batches.pop().expect("one batch");
        assert_eq!(batch.users_upsert.len(), 1);
        assert_eq!(
            batch.users_upsert.get(&user_id).expect("merged user").name,
            "alice-renamed"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delete_overrides_pending_upsert() {
        let (tx, rx) = storage_write_channel(32);
        let sink = Arc::new(MemorySink::new());
        let handle = spawn_storage_write_worker(
            sink.clone(),
            rx,
            StorageWriteWorkerConfig {
                max_batch_size: 128,
                aggregate_window: Duration::from_millis(50),
            },
        );

        let id = next_test_id();
        tx.enqueue(StorageWriteEvent::UpsertProvider(ProviderWrite {
            id,
            name: "p".to_string(),
            channel: "claude".to_string(),
            settings_json: "{}".to_string(),
            dispatch_json: "{\"rules\":[]}".to_string(),
            enabled: true,
        }))
        .await
        .expect("enqueue upsert provider");
        tx.enqueue(StorageWriteEvent::DeleteProvider { id })
            .await
            .expect("enqueue delete provider");
        drop(tx);

        handle.await.expect("worker join").expect("worker run");
        let mut batches = sink.batches.lock().expect("batch lock");
        assert_eq!(batches.len(), 1);
        let batch = batches.pop().expect("one batch");
        assert!(batch.providers_upsert.is_empty());
        assert!(batch.providers_delete.contains(&id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn credential_status_delete_overrides_pending_upsert() {
        let (tx, rx) = storage_write_channel(32);
        let sink = Arc::new(MemorySink::new());
        let handle = spawn_storage_write_worker(
            sink.clone(),
            rx,
            StorageWriteWorkerConfig {
                max_batch_size: 128,
                aggregate_window: Duration::from_millis(50),
            },
        );

        let status_id = next_test_id();
        let credential_id = next_test_id();
        let channel = "claude".to_string();
        tx.enqueue(StorageWriteEvent::UpsertCredentialStatus(
            CredentialStatusWrite {
                id: Some(status_id),
                credential_id,
                channel: channel.clone(),
                health_kind: "partial".to_string(),
                health_json: Some("[]".to_string()),
                checked_at_unix_ms: Some(1_700_000_000_000),
                last_error: Some("rate limit".to_string()),
            },
        ))
        .await
        .expect("enqueue upsert credential status");
        tx.enqueue(StorageWriteEvent::DeleteCredentialStatus { id: status_id })
            .await
            .expect("enqueue delete credential status");
        drop(tx);

        handle.await.expect("worker join").expect("worker run");
        let mut batches = sink.batches.lock().expect("batch lock");
        assert_eq!(batches.len(), 1);
        let batch = batches.pop().expect("one batch");
        assert!(batch.credential_statuses_upsert.is_empty());
        assert!(batch.credential_statuses_delete.contains(&status_id));
    }
}

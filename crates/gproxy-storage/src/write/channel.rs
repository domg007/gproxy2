use tokio::sync::mpsc;

use super::event::StorageWriteEvent;

#[derive(Debug, thiserror::Error)]
pub enum StorageWriteQueueError {
    #[error("storage write queue is closed")]
    Closed,
}

#[derive(Clone)]
pub struct StorageWriteSender {
    inner: mpsc::Sender<StorageWriteEvent>,
}

impl StorageWriteSender {
    pub async fn enqueue(&self, event: StorageWriteEvent) -> Result<(), StorageWriteQueueError> {
        self.inner
            .send(event)
            .await
            .map_err(|_| StorageWriteQueueError::Closed)
    }
}

pub struct StorageWriteReceiver {
    inner: mpsc::Receiver<StorageWriteEvent>,
}

impl StorageWriteReceiver {
    pub(crate) async fn recv(&mut self) -> Option<StorageWriteEvent> {
        self.inner.recv().await
    }
}

pub fn storage_write_channel(capacity: usize) -> (StorageWriteSender, StorageWriteReceiver) {
    let (tx, rx) = mpsc::channel(capacity);
    (
        StorageWriteSender { inner: tx },
        StorageWriteReceiver { inner: rx },
    )
}

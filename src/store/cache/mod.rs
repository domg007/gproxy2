//! Pluggable cache backend abstraction.

use std::time::Duration;

use async_trait::async_trait;

pub mod memory;

pub use memory::MemoryCache;

/// A pluggable cache backend. Single-instance deployments use
/// [`MemoryCache`]; multi-instance deployments will use a Redis-backed
/// impl (later phase). Business code depends only on this trait.
#[async_trait]
pub trait CacheBackend: Send + Sync {
    /// Fetch raw bytes for `key`, or `None` if absent or expired.
    async fn get(&self, key: &str) -> Option<Vec<u8>>;

    /// Store `value` under `key` with an optional time-to-live.
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>);

    /// Atomically add `delta` to the integer at `key` (missing = 0) and
    /// return the new value. Used by rate-limit / quota counters.
    async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> i64;

    /// Remove `key` if present.
    async fn delete(&self, key: &str);
}

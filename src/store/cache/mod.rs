//! Pluggable cache backend abstraction.

use std::time::Duration;

use async_trait::async_trait;

pub mod memory;

pub use memory::MemoryCache;

/// A pluggable cache backend. Single-instance deployments use
/// [`MemoryCache`]; multi-instance deployments will use a Redis-backed
/// impl (later phase). Business code depends only on this trait.
///
/// This is a best-effort cache layer. The in-memory impl is infallible.
/// Whether write methods should return `Result` (to surface Redis I/O errors)
/// will be decided when the Redis impl is added.
#[async_trait]
pub trait CacheBackend: Send + Sync {
    /// Fetch raw bytes for `key`, or `None` if absent or expired.
    async fn get(&self, key: &str) -> Option<Vec<u8>>;

    /// Store `value` under `key` with an optional time-to-live.
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>);

    /// Atomically add `delta` to the integer at `key` (missing = 0) and
    /// return the new value. Used by rate-limit / quota counters.
    /// The `ttl` is applied only when the key does not yet exist (or has
    /// expired); it is NOT refreshed on an already-live key (matches Redis
    /// INCR + EXPIRE-on-create semantics).
    async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> i64;

    /// Remove `key` if present.
    async fn delete(&self, key: &str);
}

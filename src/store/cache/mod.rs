//! Pluggable cache backend abstraction.
//!
//! Two implementations are provided:
//! - [`MemoryCache`] — in-process, no external dependencies; single-instance
//!   deployments.
//! - [`RedisCache`] — Redis-backed; multi-instance / shared cache.
//!
//! Business code depends only on [`CacheBackend`]; the concrete impl is
//! selected at startup based on `CacheConfig`.

use std::time::Duration;

use async_trait::async_trait;

pub mod memory;
pub mod redis;

pub use memory::MemoryCache;
pub use redis::RedisCache;

/// A pluggable cache backend.
///
/// There are two impls: [`MemoryCache`] (in-process) and
/// [`RedisCache`] (Redis-backed). Business code calls only this trait.
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
    ///
    /// Backends that can fail (e.g. Redis) return `0` on error (fail-open) after
    /// logging; callers that make allow/deny decisions on the result should
    /// account for this — a returned `0` may mean "backend unavailable", not
    /// "no increments seen".
    async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> i64;

    /// Remove `key` if present.
    async fn delete(&self, key: &str);
}

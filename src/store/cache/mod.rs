//! Pluggable cache backend abstraction.
//!
//! Native implementations:
//! - [`MemoryCache`] — in-process, no external dependencies; single-instance deployments.
//! - [`RedisCache`] — Redis-backed; multi-instance / shared cache.
//!
//! Edge (wasm32) implementations:
//! - [`LibsqlCache`] — libSQL/Turso HTTP-backed kv table.
//! - [`UpstashCache`] — Upstash Redis REST API.
//!
//! Business code depends only on [`CacheBackend`]; the concrete impl is
//! selected at startup based on `CacheConfig`.

use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub mod memory;
#[cfg(not(target_arch = "wasm32"))]
pub mod redis;

#[cfg(target_arch = "wasm32")]
pub mod b64;
#[cfg(target_arch = "wasm32")]
pub mod libsql;
#[cfg(target_arch = "wasm32")]
pub mod upstash;

#[cfg(not(target_arch = "wasm32"))]
pub use memory::MemoryCache;
#[cfg(not(target_arch = "wasm32"))]
pub use redis::RedisCache;

#[cfg(target_arch = "wasm32")]
pub use libsql::LibsqlCache;
#[cfg(target_arch = "wasm32")]
pub use upstash::UpstashCache;

/// A pluggable cache backend.
///
/// Native impls: [`MemoryCache`] (in-process) and [`RedisCache`] (Redis-backed).
/// Edge impls: [`LibsqlCache`] (libSQL/Turso) and [`UpstashCache`] (Upstash REST).
/// Business code calls only this trait.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
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

//! Redis-backed [`CacheBackend`] using a multiplexed connection manager.

use std::time::Duration;

use async_trait::async_trait;
use redis::AsyncCommands;

use super::CacheBackend;

/// Redis-backed cache. Requires a running Redis server.
///
/// Uses [`redis::aio::ConnectionManager`] for automatic reconnection and
/// multiplexed access — safe to clone and share across tasks.
///
/// # TTL-on-create semantics
///
/// `incr`'s TTL is applied only when the key is newly created (i.e. the
/// incremented value equals `delta`). This matches [`MemoryCache`](super::MemoryCache)'s
/// behaviour: an already-live key's TTL is never refreshed by `incr`.
pub struct RedisCache {
    cm: redis::aio::ConnectionManager,
}

impl RedisCache {
    /// Open a connection manager to the Redis server at `url`.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)
            .map_err(|e| anyhow::anyhow!("redis client open failed: {e}"))?;
        let cm = redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|e| anyhow::anyhow!("redis connection manager failed: {e}"))?;
        Ok(Self { cm })
    }

    /// Verify connectivity with a `PING`.
    pub async fn health(&self) -> anyhow::Result<()> {
        let mut cm = self.cm.clone();
        let _: String = redis::cmd("PING")
            .query_async(&mut cm)
            .await
            .map_err(|e| anyhow::anyhow!("redis ping failed: {e}"))?;
        Ok(())
    }
}

#[async_trait]
impl CacheBackend for RedisCache {
    async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let mut cm = self.cm.clone();
        cm.get::<_, Option<Vec<u8>>>(key).await.ok().flatten()
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) {
        let mut cm = self.cm.clone();
        if let Some(d) = ttl {
            let ms = d.as_millis() as u64;
            let _: Result<(), _> = cm.pset_ex(key, value, ms).await;
        } else {
            let _: Result<(), _> = cm.set(key, value).await;
        }
    }

    async fn delete(&self, key: &str) {
        let mut cm = self.cm.clone();
        let _: Result<u64, _> = cm.del(key).await;
    }

    /// Atomically increment `key` by `delta`. TTL is applied only on creation.
    ///
    /// Lua script ensures the TTL is set in the same round-trip as INCRBY, and
    /// only when the resulting value equals `delta` (i.e. the key was just
    /// created). This matches `MemoryCache`'s TTL-on-create heuristic.
    async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> i64 {
        // local v = redis.call('INCRBY', KEYS[1], ARGV[1])
        // if v == tonumber(ARGV[1]) and tonumber(ARGV[2]) > 0 then
        //   redis.call('PEXPIRE', KEYS[1], ARGV[2])
        // end
        // return v
        static INCR_SCRIPT: std::sync::OnceLock<redis::Script> = std::sync::OnceLock::new();
        let script = INCR_SCRIPT.get_or_init(|| {
            redis::Script::new(
                "local v = redis.call('INCRBY', KEYS[1], ARGV[1])\n\
                 if v == tonumber(ARGV[1]) and tonumber(ARGV[2]) > 0 then\n\
                   redis.call('PEXPIRE', KEYS[1], ARGV[2])\n\
                 end\n\
                 return v",
            )
        });

        let ttl_ms: i64 = ttl.map(|d| d.as_millis() as i64).unwrap_or(0);
        let mut cm = self.cm.clone();
        script
            .key(key)
            .arg(delta)
            .arg(ttl_ms)
            .invoke_async::<i64>(&mut cm)
            .await
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // This test requires a live Redis server and is skipped in normal CI.
    // Set GPROXY_TEST_REDIS_URL (e.g. redis://127.0.0.1:6379) to run it.
    #[tokio::test]
    #[ignore = "requires live Redis server via GPROXY_TEST_REDIS_URL"]
    async fn integration_set_get_incr() {
        let url = std::env::var("GPROXY_TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let cache = RedisCache::connect(&url).await.expect("connect");
        cache.health().await.expect("health");

        let key = "gproxy_test_integration";
        cache.set(key, b"hello".to_vec(), None).await;
        assert_eq!(cache.get(key).await, Some(b"hello".to_vec()));

        cache.delete(key).await;
        assert_eq!(cache.get(key).await, None);

        let ctr = "gproxy_test_counter";
        cache.delete(ctr).await;
        assert_eq!(cache.incr(ctr, 1, None).await, 1);
        assert_eq!(cache.incr(ctr, 4, None).await, 5);
        cache.delete(ctr).await;
    }
}

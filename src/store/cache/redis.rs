//! Redis-backed [`CacheBackend`] using a multiplexed connection manager.

use std::time::Duration;

use async_trait::async_trait;
use redis::AsyncCommands;

use super::{CacheBackend, InvalidationHandler};

/// Redis-backed cache. Requires a running Redis server.
///
/// Uses [`redis::aio::ConnectionManager`] for automatic reconnection and
/// multiplexed access — safe to clone and share across tasks.
///
/// # TTL-on-create semantics
///
/// `incr`'s TTL is applied only when the key did not previously exist, detected
/// via a preceding `EXISTS` call inside the Lua script. This ensures the window
/// expiry is never reset by subsequent increments (unlike the old `v == delta`
/// heuristic, which misfired whenever a live counter happened to equal `delta`).
pub struct RedisCache {
    /// Kept for opening dedicated pub/sub connections (a `ConnectionManager`
    /// multiplexes and cannot enter subscribe mode).
    client: redis::Client,
    cm: redis::aio::ConnectionManager,
}

impl RedisCache {
    /// Open a connection manager to the Redis server at `url`.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)
            .map_err(|e| anyhow::anyhow!("redis client open failed: {e}"))?;
        let cm = redis::aio::ConnectionManager::new(client.clone())
            .await
            .map_err(|e| anyhow::anyhow!("redis connection manager failed: {e}"))?;
        Ok(Self { client, cm })
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
            // Duration::ZERO treated as no-expiry (PSETEX 0 is rejected by Redis).
            let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
            if ms == 0 {
                let _: Result<(), _> = cm.set(key, value).await;
            } else {
                let _: Result<(), _> = cm.pset_ex(key, value, ms).await;
            }
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
    /// Uses an EXISTS-before-INCRBY Lua script so the TTL is set in the same
    /// round-trip as INCRBY, and only when the key did not previously exist —
    /// not whenever the counter value happens to equal `delta`. This prevents
    /// spurious TTL resets on live counters (e.g. two increments of the same
    /// delta on a pre-existing key).
    ///
    /// On any Redis/Lua error this returns `0` (fail-open) after logging;
    /// callers making allow/deny decisions on the result should account for this.
    async fn incr(&self, key: &str, delta: i64, ttl: Option<Duration>) -> i64 {
        // local exists = redis.call('EXISTS', KEYS[1])
        // local v = redis.call('INCRBY', KEYS[1], ARGV[1])
        // if exists == 0 and tonumber(ARGV[2]) > 0 then
        //   redis.call('PEXPIRE', KEYS[1], ARGV[2])
        // end
        // return v
        static INCR_SCRIPT: std::sync::OnceLock<redis::Script> = std::sync::OnceLock::new();
        let script = INCR_SCRIPT.get_or_init(|| {
            redis::Script::new(
                "local exists = redis.call('EXISTS', KEYS[1])\n\
                 local v = redis.call('INCRBY', KEYS[1], ARGV[1])\n\
                 if exists == 0 and tonumber(ARGV[2]) > 0 then\n\
                   redis.call('PEXPIRE', KEYS[1], ARGV[2])\n\
                 end\n\
                 return v",
            )
        });

        let ttl_ms: i64 = ttl
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let mut cm = self.cm.clone();
        script
            .key(key)
            .arg(delta)
            .arg(ttl_ms)
            .invoke_async::<i64>(&mut cm)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("redis incr failed, returning 0 (fail-open): {e}");
                0
            })
    }

    /// Publish `payload` to `channel`. Best-effort: a failed publish is logged
    /// but never propagated, so an invalidation hiccup can't break the caller.
    async fn publish(&self, channel: &str, payload: &[u8]) {
        let mut cm = self.cm.clone();
        let r: Result<i64, _> = redis::cmd("PUBLISH")
            .arg(channel)
            .arg(payload)
            .query_async(&mut cm)
            .await;
        if let Err(e) = r {
            tracing::warn!(error = %e, channel, "redis publish failed");
        }
    }

    /// Subscribe to `channel`, invoking `handler` for each message until the
    /// connection drops. Blocks for the lifetime of the subscription, so the
    /// caller is expected to spawn this. Reconnection on drop is a follow-up.
    async fn subscribe(&self, channel: &str, handler: InvalidationHandler) {
        let mut pubsub = match self.client.get_async_pubsub().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "redis pubsub connect failed");
                return;
            }
        };
        if let Err(e) = pubsub.subscribe(channel).await {
            tracing::warn!(error = %e, channel, "redis subscribe failed");
            return;
        }
        let mut stream = pubsub.on_message();
        use futures_util::StreamExt;
        while let Some(msg) = stream.next().await {
            handler(msg.get_payload_bytes().to_vec());
        }
        tracing::warn!(channel, "redis subscription ended (connection dropped)");
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

    /// Round-trips one message through real PUBLISH/SUBSCRIBE. Skips (does not
    /// fail) when no Redis is reachable, so it's safe to run unconditionally in
    /// CI without a Redis service.
    #[tokio::test]
    async fn redis_publish_subscribe_roundtrip() {
        let Ok(cache) = RedisCache::connect("redis://127.0.0.1:6379").await else {
            eprintln!("skipping: no redis at 127.0.0.1:6379");
            return;
        };
        if cache.health().await.is_err() {
            eprintln!("skipping: redis unreachable");
            return;
        }

        let channel = "gproxy_test_pubsub";
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
        let tx = std::sync::Mutex::new(Some(tx));
        let sub = {
            let cache = RedisCache::connect("redis://127.0.0.1:6379")
                .await
                .expect("subscriber connect");
            tokio::spawn(async move {
                cache
                    .subscribe(
                        channel,
                        Box::new(move |payload| {
                            if let Some(tx) = tx.lock().unwrap().take() {
                                let _ = tx.send(payload);
                            }
                        }),
                    )
                    .await;
            })
        };

        // Give the subscription time to establish before publishing.
        tokio::time::sleep(Duration::from_millis(200)).await;
        cache.publish(channel, b"ping").await;

        let got = tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("handler did not receive within 1s")
            .expect("sender dropped");
        assert_eq!(got, b"ping");
        sub.abort();
    }
}

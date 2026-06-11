//! Admin OAuth login orchestration (§14.5): short-TTL PKCE/state in the cache.
//!
//! The interactive authcode flow is two HTTP round-trips (`start` → user visits
//! the authorize URL → `complete` with the callback). Between them the PKCE
//! verifier + CSRF state + redirect_uri must survive server-side but never reach
//! the browser. They live in the cache under a random one-shot id with a 10-min
//! TTL — `take` deletes on read so a callback can't be replayed.
//!
//! cache-only, axum-free → compiles on native and edge.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::store::cache::CacheBackend;

/// Lifetime of a pending login (matches the v1 codex state TTL).
const LOGIN_TTL: Duration = Duration::from_secs(600);

/// Server-side state of a pending OAuth login, keyed by an opaque session id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginSession {
    pub channel: String,
    pub verifier: String,
    pub state: String,
    pub redirect_uri: String,
}

/// Stash a pending login → cache `login:{sid}` for [`LOGIN_TTL`]. Returns the
/// random one-shot session id.
pub async fn start(
    cache: &dyn CacheBackend,
    channel: String,
    verifier: String,
    state: String,
    redirect_uri: String,
) -> String {
    let sid = crate::util::rand::uuid_v4();
    let session = LoginSession {
        channel,
        verifier,
        state,
        redirect_uri,
    };
    // Serialization of this fixed shape cannot fail; an empty value would just
    // make `take` miss (a benign 400 later), so swallow the impossible error.
    if let Ok(bytes) = serde_json::to_vec(&session) {
        cache.set(&key(&sid), bytes, Some(LOGIN_TTL)).await;
    }
    sid
}

/// Consume a pending login (one-shot: get + delete). `None` if missing/expired.
pub async fn take(cache: &dyn CacheBackend, sid: &str) -> Option<LoginSession> {
    let raw = cache.get(&key(sid)).await?;
    cache.delete(&key(sid)).await;
    serde_json::from_slice(&raw).ok()
}

fn key(sid: &str) -> String {
    format!("login:{sid}")
}

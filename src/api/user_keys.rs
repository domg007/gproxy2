//! User-key DTOs that keep the bare key and its ciphertext off the wire.
//!
//! [`UserKeyView`] exposes only a short `key_prefix` (first 8 chars of the
//! digest) on reads; [`UserKeyUpsert`] takes the BARE api key on writes, which
//! the handler digests + seals (or, when omitted on an update, leaves the
//! stored digest/ciphertext untouched).

use crate::store::persistence::records::UserKey;

/// Read-side user-key shape: identity fields plus a short non-reversible
/// `key_prefix`. The bare key and its ciphertext are never serialized.
#[derive(serde::Serialize)]
pub struct UserKeyView {
    pub id: i64,
    pub user_id: i64,
    pub label: Option<String>,
    pub enabled: bool,
    /// First 8 chars of the digest — never the bare key.
    pub key_prefix: String,
}

impl From<UserKey> for UserKeyView {
    fn from(k: UserKey) -> Self {
        let key_prefix: String = k.api_key_digest.chars().take(8).collect();
        UserKeyView {
            id: k.id,
            user_id: k.user_id,
            label: k.label,
            enabled: k.enabled,
            key_prefix,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Write-side user-key shape. `api_key` is the BARE key (digested + sealed by
/// the handler): required on create, optional on update (omit to keep the
/// stored digest/ciphertext). `id = None` creates, `Some(id)` updates.
#[derive(serde::Deserialize)]
pub struct UserKeyUpsert {
    #[serde(default)]
    pub id: Option<i64>,
    /// User id — taken from the path; ignored if present in the body.
    #[serde(default)]
    pub user_id: i64,
    /// Bare api key. `Some` → digested + sealed; `None` on update → keep
    /// existing; `None` on create → rejected (400).
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

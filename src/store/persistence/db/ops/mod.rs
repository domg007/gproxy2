//! Trait-method implementations for the `db` backend (SeaORM ↔ records).

pub mod authz;
pub mod identity;
pub mod logs;
pub mod provider;
pub mod routing;
pub mod settings;
pub mod tokenize;
pub mod transform;
pub mod usage;

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

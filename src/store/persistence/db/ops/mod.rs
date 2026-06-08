//! Trait-method implementations for the `db` backend (SeaORM ↔ records).

pub mod identity;
pub mod provider;
pub mod routing;
pub mod rules;
pub mod settings;
pub mod usage;

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

//! Trait-method implementations for the `db` backend (SeaORM ↔ records).

pub mod aliases;
pub mod credential_statuses;
pub mod credentials;
pub mod provider_models;
pub mod providers;
pub mod route_members;
pub mod routes;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

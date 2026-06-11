//! Trait-method implementations for the `db` backend (SeaORM ↔ records).
//!
//! Config-entity `upsert`s insert WITH an explicit id when a `Some(id)` row is
//! missing (seeding an empty store from a pinned import bundle — matches the
//! file backend). Caveat: on Postgres an explicit-PK insert does NOT advance
//! the identity sequence, so a LATER auto-id insert (admin API, M10) could
//! collide. Import seeding is collision-free (a fully-pinned bundle into an
//! EMPTY store → every insert is explicit); post-seed sequence sync on Postgres
//! is an M10/admin-API concern. SQLite self-heals (next auto id is max+1).

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

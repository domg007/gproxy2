//! Versioned schema migration framework, shared by the two SQL backends
//! (`db` SeaORM + `libsql` edge). The execution differs per backend (SeaORM
//! raw SQL vs. hand SQL over `LibsqlClient::execute`), but the *migration list*
//! and the version-ordering logic live here, once.
//!
//! ## Model
//!
//! A `schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER)` table
//! tracks which migrations have been applied. On connect each SQL backend:
//!
//!   1. runs its existing `CREATE TABLE IF NOT EXISTS` baseline (entity-driven
//!      for `db`, the hand `TABLES` list for `libsql`) — this lands a *fresh*
//!      DB at the current schema, and is a no-op on an already-created DB;
//!   2. ensures `schema_migrations` exists;
//!   3. reads `MAX(version)` (0 when the table is empty);
//!   4. if the table is empty, **stamps the baseline** ([`BASELINE_VERSION`])
//!      without running any SQL — both a fresh DB and a DB created by the old
//!      auto-create already have the baseline tables, so re-running destructive
//!      steps must be avoided;
//!   5. applies every [`MIGRATIONS`] entry with `version >` the stamped max, in
//!      ascending order, recording each in `schema_migrations`.
//!
//! Idempotent and safe to run on every boot.
//!
//! ## Adding a migration
//!
//! Append a [`Migration`] to [`MIGRATIONS`] with the next integer version (keep
//! the list sorted ascending). Use dialect-portable SQL (sqlite/pg/mysql) since
//! the same `sql` runs on both backends. Example:
//!
//! ```ignore
//! Migration {
//!     version: 2,
//!     description: "add providers.region",
//!     sql: &["ALTER TABLE providers ADD COLUMN region TEXT"],
//! }
//! ```

/// The version stamped for the auto-created baseline schema. Migrations in
/// [`MIGRATIONS`] must use versions strictly greater than this.
pub const BASELINE_VERSION: i64 = 1;

/// A single ordered schema migration. `sql` may hold multiple statements, each
/// executed in list order (one statement per backend call).
pub struct Migration {
    /// Strictly-increasing version; must be `> BASELINE_VERSION` and unique.
    pub version: i64,
    /// Human-readable note for diagnostics.
    pub description: &'static str,
    /// Statements to run, in order. Keep dialect-portable (sqlite/pg/mysql).
    pub sql: &'static [&'static str],
}

/// Portable DDL for the bookkeeping table. `INTEGER` is accepted by all three
/// dialects; no autoincrement needed (the version is supplied explicitly).
pub const CREATE_MIGRATIONS_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (\
     version INTEGER PRIMARY KEY, \
     applied_at INTEGER NOT NULL)";

/// Highest applied version, reading the bookkeeping table.
pub const SELECT_MAX_VERSION: &str = "SELECT COALESCE(MAX(version), 0) AS v FROM schema_migrations";

/// Ordered list of migrations to apply *after* the baseline. Append new
/// entries here (see the module docs). The entry below is the representative
/// placeholder showing the pattern; it is a comment-only no-op (`sql: &[]`) so
/// it records the version without altering the schema. Replace its `sql` (or
/// add a new entry) for the next real change.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 2,
    description: "placeholder: no-op example migration (replace with real DDL)",
    sql: &[],
}];

/// Migrations with `version > current`, in ascending order — the work a runner
/// must apply. Pulled out as a pure function so the ordering logic is unit-
/// testable without a database.
pub fn pending(current: i64) -> Vec<&'static Migration> {
    let mut out: Vec<&Migration> = MIGRATIONS.iter().filter(|m| m.version > current).collect();
    out.sort_by_key(|m| m.version);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_filters_and_orders_by_version() {
        // From a fresh baseline stamp, every listed migration is pending.
        let all = pending(BASELINE_VERSION);
        assert_eq!(
            all.iter().map(|m| m.version).collect::<Vec<_>>(),
            MIGRATIONS.iter().map(|m| m.version).collect::<Vec<_>>(),
        );
        // Versions are strictly ascending and all above the baseline.
        let mut prev = BASELINE_VERSION;
        for m in &all {
            assert!(m.version > prev, "versions must strictly ascend");
            assert!(m.version > BASELINE_VERSION, "must be above baseline");
            prev = m.version;
        }
        // Already at the top → nothing pending.
        let top = MIGRATIONS
            .iter()
            .map(|m| m.version)
            .max()
            .unwrap_or(BASELINE_VERSION);
        assert!(pending(top).is_empty());
    }
}

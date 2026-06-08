//! Durable storage abstraction.
//!
//! Native impls: `file` (local disk, single-instance) and `db` (SeaORM, multi-instance).
//! Edge (wasm32) impl: `libsql` (libSQL/Turso over Hrana HTTP).
//! Domain code calls only trait methods.

#[cfg(all(not(target_arch = "wasm32"), feature = "persist-db"))]
pub mod db;
#[cfg(all(not(target_arch = "wasm32"), feature = "persist-file"))]
pub mod file;

#[cfg(all(target_arch = "wasm32", feature = "persist-libsql"))]
pub mod libsql;

pub mod records;

#[cfg(all(not(target_arch = "wasm32"), feature = "persist-db"))]
pub use db::DbPersistence;
#[cfg(all(not(target_arch = "wasm32"), feature = "persist-file"))]
pub use file::FilePersistence;

#[cfg(all(target_arch = "wasm32", feature = "persist-libsql"))]
pub use libsql::LibsqlPersistence;

use records::{
    Alias, AliasInput, BetaHeader, BetaHeaderInput, CacheBreakpoint, CacheBreakpointInput,
    Credential, CredentialInput, CredentialStatus, CredentialStatusInput, DownstreamRequest,
    DownstreamRequestInput, InstanceSettings, InstanceSettingsInput, Org, OrgInput, PreludeSystem,
    PreludeSystemInput, Provider, ProviderInput, ProviderModel, ProviderModelInput, Quota,
    QuotaInput, RateLimit, RateLimitInput, RewriteRule, RewriteRuleInput, Route, RouteInput,
    RouteMember, RouteMemberInput, RoutePermission, RoutePermissionInput, RoutingRule,
    RoutingRuleInput, SanitizeRule, SanitizeRuleInput, Team, TeamInput, UpstreamRequest,
    UpstreamRequestInput, Usage, UsageInput, UsageRollup, UsageRollupInput, User, UserInput,
    UserKey, UserKeyInput,
};

/// Durable storage abstraction.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait PersistenceBackend: Send + Sync {
    /// Backend kind label for diagnostics: "file" | "db" | "libsql".
    fn kind(&self) -> &'static str;

    /// Verify the backend is reachable/usable.
    async fn health(&self) -> anyhow::Result<()>;

    // ── providers (§8-B) ────────────────────────────────────────────────────

    /// List all providers.
    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>>;

    /// Fetch a provider by id, or `None` if absent.
    async fn get_provider(&self, id: i64) -> anyhow::Result<Option<Provider>>;

    /// Fetch a provider by its unique name, or `None` if absent.
    async fn get_provider_by_name(&self, name: &str) -> anyhow::Result<Option<Provider>>;

    /// Insert (`input.id == None`) or update (`Some(id)`) a provider; returns
    /// the stored record with id and timestamps populated.
    async fn upsert_provider(&self, input: ProviderInput) -> anyhow::Result<Provider>;

    /// Delete a provider by id; returns whether a row was removed.
    /// Application-level cascade: also removes the provider's credentials
    /// (and their statuses) and provider models.
    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool>;

    // ── credentials (§8-B) ──────────────────────────────────────────────────

    /// List credentials in a provider's pool.
    async fn list_credentials(&self, provider_id: i64) -> anyhow::Result<Vec<Credential>>;

    /// Fetch a credential by id.
    async fn get_credential(&self, id: i64) -> anyhow::Result<Option<Credential>>;

    /// Insert or update a credential.
    async fn upsert_credential(&self, input: CredentialInput) -> anyhow::Result<Credential>;

    /// Delete a credential; cascades to its status snapshots.
    async fn delete_credential(&self, id: i64) -> anyhow::Result<bool>;

    /// List a credential's health snapshots.
    async fn list_credential_statuses(
        &self,
        credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>>;

    /// Insert or update a credential status (unique per `(credential_id, channel)`).
    async fn upsert_credential_status(
        &self,
        input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus>;

    /// Delete a credential status by id.
    async fn delete_credential_status(&self, id: i64) -> anyhow::Result<bool>;

    // ── routes / members / aliases (§8-A) ───────────────────────────────────

    /// List all routes.
    async fn list_routes(&self) -> anyhow::Result<Vec<Route>>;

    /// Fetch a route by id.
    async fn get_route(&self, id: i64) -> anyhow::Result<Option<Route>>;

    /// Fetch a route by its unique name.
    async fn get_route_by_name(&self, name: &str) -> anyhow::Result<Option<Route>>;

    /// Insert or update a route.
    async fn upsert_route(&self, input: RouteInput) -> anyhow::Result<Route>;

    /// Delete a route; cascades to its members and aliases.
    async fn delete_route(&self, id: i64) -> anyhow::Result<bool>;

    /// List a route's members.
    async fn list_route_members(&self, route_id: i64) -> anyhow::Result<Vec<RouteMember>>;

    /// Insert or update a route member.
    async fn upsert_route_member(&self, input: RouteMemberInput) -> anyhow::Result<RouteMember>;

    /// Delete a route member by id.
    async fn delete_route_member(&self, id: i64) -> anyhow::Result<bool>;

    /// List all aliases.
    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>>;

    /// Fetch an alias by its unique alias name.
    async fn get_alias_by_name(&self, alias: &str) -> anyhow::Result<Option<Alias>>;

    /// Insert or update an alias.
    async fn upsert_alias(&self, input: AliasInput) -> anyhow::Result<Alias>;

    /// Delete an alias by id.
    async fn delete_alias(&self, id: i64) -> anyhow::Result<bool>;

    // ── provider models (§8-A) ──────────────────────────────────────────────

    /// List a provider's models.
    async fn list_provider_models(&self, provider_id: i64) -> anyhow::Result<Vec<ProviderModel>>;

    /// Insert or update a provider model.
    async fn upsert_provider_model(
        &self,
        input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel>;

    /// Delete a provider model by id.
    async fn delete_provider_model(&self, id: i64) -> anyhow::Result<bool>;

    // ── routing rules (§8-B) ─────────────────────────────────────────────────

    /// List a provider's routing rules.
    async fn list_routing_rules(&self, provider_id: i64) -> anyhow::Result<Vec<RoutingRule>>;

    /// Fetch a routing rule by id.
    async fn get_routing_rule(&self, id: i64) -> anyhow::Result<Option<RoutingRule>>;

    /// Insert or update a routing rule (unique per `(provider_id, operation, kind)`).
    async fn upsert_routing_rule(&self, input: RoutingRuleInput) -> anyhow::Result<RoutingRule>;

    /// Delete a routing rule by id.
    async fn delete_routing_rule(&self, id: i64) -> anyhow::Result<bool>;

    // ── rewrite rules (§8-B) ─────────────────────────────────────────────────

    /// List a provider's rewrite rules.
    async fn list_rewrite_rules(&self, provider_id: i64) -> anyhow::Result<Vec<RewriteRule>>;

    /// Fetch a rewrite rule by id.
    async fn get_rewrite_rule(&self, id: i64) -> anyhow::Result<Option<RewriteRule>>;

    /// Insert or update a rewrite rule.
    async fn upsert_rewrite_rule(&self, input: RewriteRuleInput) -> anyhow::Result<RewriteRule>;

    /// Delete a rewrite rule by id.
    async fn delete_rewrite_rule(&self, id: i64) -> anyhow::Result<bool>;

    // ── sanitize rules (§8-B) ────────────────────────────────────────────────

    /// List a provider's sanitize rules.
    async fn list_sanitize_rules(&self, provider_id: i64) -> anyhow::Result<Vec<SanitizeRule>>;

    /// Fetch a sanitize rule by id.
    async fn get_sanitize_rule(&self, id: i64) -> anyhow::Result<Option<SanitizeRule>>;

    /// Insert or update a sanitize rule.
    async fn upsert_sanitize_rule(&self, input: SanitizeRuleInput) -> anyhow::Result<SanitizeRule>;

    /// Delete a sanitize rule by id.
    async fn delete_sanitize_rule(&self, id: i64) -> anyhow::Result<bool>;

    // ── cache breakpoints (§8-B) ─────────────────────────────────────────────

    /// List a provider's cache breakpoints.
    async fn list_cache_breakpoints(
        &self,
        provider_id: i64,
    ) -> anyhow::Result<Vec<CacheBreakpoint>>;

    /// Fetch a cache breakpoint by id.
    async fn get_cache_breakpoint(&self, id: i64) -> anyhow::Result<Option<CacheBreakpoint>>;

    /// Insert or update a cache breakpoint.
    async fn upsert_cache_breakpoint(
        &self,
        input: CacheBreakpointInput,
    ) -> anyhow::Result<CacheBreakpoint>;

    /// Delete a cache breakpoint by id.
    async fn delete_cache_breakpoint(&self, id: i64) -> anyhow::Result<bool>;

    // ── beta headers (§8-B) ──────────────────────────────────────────────────

    /// List a provider's beta headers.
    async fn list_beta_headers(&self, provider_id: i64) -> anyhow::Result<Vec<BetaHeader>>;

    /// Fetch a beta header by id.
    async fn get_beta_header(&self, id: i64) -> anyhow::Result<Option<BetaHeader>>;

    /// Insert or update a beta header.
    async fn upsert_beta_header(&self, input: BetaHeaderInput) -> anyhow::Result<BetaHeader>;

    /// Delete a beta header by id.
    async fn delete_beta_header(&self, id: i64) -> anyhow::Result<bool>;

    // ── system preludes (§8-B) ───────────────────────────────────────────────

    /// List a provider's system preludes.
    async fn list_preludes_system(&self, provider_id: i64) -> anyhow::Result<Vec<PreludeSystem>>;

    /// Fetch a system prelude by id.
    async fn get_prelude_system(&self, id: i64) -> anyhow::Result<Option<PreludeSystem>>;

    /// Insert or update a system prelude.
    async fn upsert_prelude_system(
        &self,
        input: PreludeSystemInput,
    ) -> anyhow::Result<PreludeSystem>;

    /// Delete a system prelude by id.
    async fn delete_prelude_system(&self, id: i64) -> anyhow::Result<bool>;

    // ── orgs (§8-C) ─────────────────────────────────────────────────────────

    /// List all orgs.
    async fn list_orgs(&self) -> anyhow::Result<Vec<Org>>;

    /// Fetch an org by id.
    async fn get_org(&self, id: i64) -> anyhow::Result<Option<Org>>;

    /// Fetch an org by its unique name.
    async fn get_org_by_name(&self, name: &str) -> anyhow::Result<Option<Org>>;

    /// Insert or update an org.
    async fn upsert_org(&self, input: OrgInput) -> anyhow::Result<Org>;

    /// Delete an org; cascades to its teams, users (and their keys), and any
    /// org-scoped route permissions, rate limits, and quotas.
    async fn delete_org(&self, id: i64) -> anyhow::Result<bool>;

    // ── teams (§8-C) ────────────────────────────────────────────────────────

    /// List a team's siblings within an org.
    async fn list_teams(&self, org_id: i64) -> anyhow::Result<Vec<Team>>;

    /// Fetch a team by id.
    async fn get_team(&self, id: i64) -> anyhow::Result<Option<Team>>;

    /// Insert or update a team (unique per `(org_id, name)`).
    async fn upsert_team(&self, input: TeamInput) -> anyhow::Result<Team>;

    /// Delete a team; detaches its members and drops team-scoped route
    /// permissions, rate limits, and quotas.
    async fn delete_team(&self, id: i64) -> anyhow::Result<bool>;

    // ── users (§8-C) ────────────────────────────────────────────────────────

    /// List all users.
    async fn list_users(&self) -> anyhow::Result<Vec<User>>;

    /// Fetch a user by id.
    async fn get_user(&self, id: i64) -> anyhow::Result<Option<User>>;

    /// Fetch a user by their unique name.
    async fn get_user_by_name(&self, name: &str) -> anyhow::Result<Option<User>>;

    /// Insert or update a user.
    async fn upsert_user(&self, input: UserInput) -> anyhow::Result<User>;

    /// Delete a user; cascades to their keys and user-scoped route permissions,
    /// rate limits, and quotas.
    async fn delete_user(&self, id: i64) -> anyhow::Result<bool>;

    // ── user keys (§8-C) ────────────────────────────────────────────────────

    /// List a user's API keys.
    async fn list_user_keys(&self, user_id: i64) -> anyhow::Result<Vec<UserKey>>;

    /// Fetch a user key by id.
    async fn get_user_key(&self, id: i64) -> anyhow::Result<Option<UserKey>>;

    /// Find a user key by its unique digest.
    async fn find_user_key_by_digest(&self, digest: &str) -> anyhow::Result<Option<UserKey>>;

    /// Insert or update a user key.
    async fn upsert_user_key(&self, input: UserKeyInput) -> anyhow::Result<UserKey>;

    /// Delete a user key by id.
    async fn delete_user_key(&self, id: i64) -> anyhow::Result<bool>;

    // ── authz: route permissions / rate limits / quotas (§8-C) ──────────────

    /// List route permissions for a scope (`org` | `team` | `user`).
    async fn list_route_permissions(
        &self,
        scope: &str,
        scope_id: i64,
    ) -> anyhow::Result<Vec<RoutePermission>>;

    /// Insert or update a route permission.
    async fn upsert_route_permission(
        &self,
        input: RoutePermissionInput,
    ) -> anyhow::Result<RoutePermission>;

    /// Delete a route permission by id.
    async fn delete_route_permission(&self, id: i64) -> anyhow::Result<bool>;

    /// List rate limits for a scope.
    async fn list_rate_limits(&self, scope: &str, scope_id: i64) -> anyhow::Result<Vec<RateLimit>>;

    /// Insert or update a rate limit.
    async fn upsert_rate_limit(&self, input: RateLimitInput) -> anyhow::Result<RateLimit>;

    /// Delete a rate limit by id.
    async fn delete_rate_limit(&self, id: i64) -> anyhow::Result<bool>;

    /// Fetch the quota for a scope (unique per `(scope, scope_id)`).
    async fn get_quota(&self, scope: &str, scope_id: i64) -> anyhow::Result<Option<Quota>>;

    /// Insert or update a quota.
    async fn upsert_quota(&self, input: QuotaInput) -> anyhow::Result<Quota>;

    /// Delete a quota by id.
    async fn delete_quota(&self, id: i64) -> anyhow::Result<bool>;

    // ── usage / logs (§8-D) ─────────────────────────────────────────────────

    /// Append a per-request usage row (append-only).
    async fn append_usage(&self, input: UsageInput) -> anyhow::Result<Usage>;

    /// List the most recent usage rows (by id desc), up to `limit`.
    async fn list_usages(&self, limit: u64) -> anyhow::Result<Vec<Usage>>;

    /// Accumulate metric deltas into the rollup bucket identified by the input's
    /// `(granularity, bucket_start, dimensions)`; creates the bucket if absent.
    async fn add_usage_rollup(&self, input: UsageRollupInput) -> anyhow::Result<UsageRollup>;

    /// List rollup buckets for `granularity` with `bucket_start` in `[from, to]`.
    async fn list_usage_rollups(
        &self,
        granularity: &str,
        from: i64,
        to: i64,
    ) -> anyhow::Result<Vec<UsageRollup>>;

    /// Append a raw downstream (client → proxy) request log entry.
    async fn append_downstream_request(
        &self,
        input: DownstreamRequestInput,
    ) -> anyhow::Result<DownstreamRequest>;

    /// List downstream request log entries for a `request_id`.
    async fn list_downstream_requests(
        &self,
        request_id: &str,
    ) -> anyhow::Result<Vec<DownstreamRequest>>;

    /// Append a raw upstream (proxy → provider) request log entry.
    async fn append_upstream_request(
        &self,
        input: UpstreamRequestInput,
    ) -> anyhow::Result<UpstreamRequest>;

    /// List upstream request log entries for a `request_id`.
    async fn list_upstream_requests(
        &self,
        request_id: &str,
    ) -> anyhow::Result<Vec<UpstreamRequest>>;

    // ── instance settings (§8) ──────────────────────────────────────────────

    /// List all instance settings.
    async fn list_instance_settings(&self) -> anyhow::Result<Vec<InstanceSettings>>;

    /// Fetch instance settings by their unique `instance_name`, or `None`.
    async fn get_instance_settings(
        &self,
        instance_name: &str,
    ) -> anyhow::Result<Option<InstanceSettings>>;

    /// Insert or update instance settings (unique per `instance_name`).
    async fn upsert_instance_settings(
        &self,
        input: InstanceSettingsInput,
    ) -> anyhow::Result<InstanceSettings>;
}

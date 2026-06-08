//! `PersistenceBackend` implementation for [`DbPersistence`].

use async_trait::async_trait;

use super::DbPersistence;
use super::ops;
use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::{
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

#[async_trait]
impl PersistenceBackend for DbPersistence {
    fn kind(&self) -> &'static str {
        "db"
    }

    async fn health(&self) -> anyhow::Result<()> {
        self.conn
            .ping()
            .await
            .map_err(|e| anyhow::anyhow!("db ping failed: {e}"))
    }

    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        ops::provider::providers::list(&self.conn).await
    }

    async fn get_provider(&self, id: i64) -> anyhow::Result<Option<Provider>> {
        ops::provider::providers::get(&self.conn, id).await
    }

    async fn get_provider_by_name(&self, name: &str) -> anyhow::Result<Option<Provider>> {
        ops::provider::providers::get_by_name(&self.conn, name).await
    }

    async fn upsert_provider(&self, input: ProviderInput) -> anyhow::Result<Provider> {
        ops::provider::providers::upsert(&self.conn, input).await
    }

    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool> {
        ops::provider::providers::delete(&self.conn, id).await
    }

    async fn list_credentials(&self, provider_id: i64) -> anyhow::Result<Vec<Credential>> {
        ops::provider::credentials::list(&self.conn, provider_id).await
    }

    async fn get_credential(&self, id: i64) -> anyhow::Result<Option<Credential>> {
        ops::provider::credentials::get(&self.conn, id).await
    }

    async fn upsert_credential(&self, input: CredentialInput) -> anyhow::Result<Credential> {
        ops::provider::credentials::upsert(&self.conn, input).await
    }

    async fn delete_credential(&self, id: i64) -> anyhow::Result<bool> {
        ops::provider::credentials::delete(&self.conn, id).await
    }

    async fn list_credential_statuses(
        &self,
        credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>> {
        ops::provider::credential_statuses::list(&self.conn, credential_id).await
    }

    async fn upsert_credential_status(
        &self,
        input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus> {
        ops::provider::credential_statuses::upsert(&self.conn, input).await
    }

    async fn delete_credential_status(&self, id: i64) -> anyhow::Result<bool> {
        ops::provider::credential_statuses::delete(&self.conn, id).await
    }

    async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        ops::routing::routes::list(&self.conn).await
    }

    async fn get_route(&self, id: i64) -> anyhow::Result<Option<Route>> {
        ops::routing::routes::get(&self.conn, id).await
    }

    async fn get_route_by_name(&self, name: &str) -> anyhow::Result<Option<Route>> {
        ops::routing::routes::get_by_name(&self.conn, name).await
    }

    async fn upsert_route(&self, input: RouteInput) -> anyhow::Result<Route> {
        ops::routing::routes::upsert(&self.conn, input).await
    }

    async fn delete_route(&self, id: i64) -> anyhow::Result<bool> {
        ops::routing::routes::delete(&self.conn, id).await
    }

    async fn list_route_members(&self, route_id: i64) -> anyhow::Result<Vec<RouteMember>> {
        ops::routing::route_members::list(&self.conn, route_id).await
    }

    async fn upsert_route_member(&self, input: RouteMemberInput) -> anyhow::Result<RouteMember> {
        ops::routing::route_members::upsert(&self.conn, input).await
    }

    async fn delete_route_member(&self, id: i64) -> anyhow::Result<bool> {
        ops::routing::route_members::delete(&self.conn, id).await
    }

    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>> {
        ops::routing::aliases::list(&self.conn).await
    }

    async fn get_alias_by_name(&self, alias: &str) -> anyhow::Result<Option<Alias>> {
        ops::routing::aliases::get_by_name(&self.conn, alias).await
    }

    async fn upsert_alias(&self, input: AliasInput) -> anyhow::Result<Alias> {
        ops::routing::aliases::upsert(&self.conn, input).await
    }

    async fn delete_alias(&self, id: i64) -> anyhow::Result<bool> {
        ops::routing::aliases::delete(&self.conn, id).await
    }

    async fn list_provider_models(&self, provider_id: i64) -> anyhow::Result<Vec<ProviderModel>> {
        ops::routing::provider_models::list(&self.conn, provider_id).await
    }

    async fn upsert_provider_model(
        &self,
        input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel> {
        ops::routing::provider_models::upsert(&self.conn, input).await
    }

    async fn delete_provider_model(&self, id: i64) -> anyhow::Result<bool> {
        ops::routing::provider_models::delete(&self.conn, id).await
    }

    async fn list_routing_rules(&self, provider_id: i64) -> anyhow::Result<Vec<RoutingRule>> {
        ops::rules::routing_rules::list(&self.conn, provider_id).await
    }

    async fn get_routing_rule(&self, id: i64) -> anyhow::Result<Option<RoutingRule>> {
        ops::rules::routing_rules::get(&self.conn, id).await
    }

    async fn upsert_routing_rule(&self, input: RoutingRuleInput) -> anyhow::Result<RoutingRule> {
        ops::rules::routing_rules::upsert(&self.conn, input).await
    }

    async fn delete_routing_rule(&self, id: i64) -> anyhow::Result<bool> {
        ops::rules::routing_rules::delete(&self.conn, id).await
    }

    async fn list_rewrite_rules(&self, provider_id: i64) -> anyhow::Result<Vec<RewriteRule>> {
        ops::rules::rewrite_rules::list(&self.conn, provider_id).await
    }

    async fn get_rewrite_rule(&self, id: i64) -> anyhow::Result<Option<RewriteRule>> {
        ops::rules::rewrite_rules::get(&self.conn, id).await
    }

    async fn upsert_rewrite_rule(&self, input: RewriteRuleInput) -> anyhow::Result<RewriteRule> {
        ops::rules::rewrite_rules::upsert(&self.conn, input).await
    }

    async fn delete_rewrite_rule(&self, id: i64) -> anyhow::Result<bool> {
        ops::rules::rewrite_rules::delete(&self.conn, id).await
    }

    async fn list_sanitize_rules(&self, provider_id: i64) -> anyhow::Result<Vec<SanitizeRule>> {
        ops::rules::sanitize_rules::list(&self.conn, provider_id).await
    }

    async fn get_sanitize_rule(&self, id: i64) -> anyhow::Result<Option<SanitizeRule>> {
        ops::rules::sanitize_rules::get(&self.conn, id).await
    }

    async fn upsert_sanitize_rule(&self, input: SanitizeRuleInput) -> anyhow::Result<SanitizeRule> {
        ops::rules::sanitize_rules::upsert(&self.conn, input).await
    }

    async fn delete_sanitize_rule(&self, id: i64) -> anyhow::Result<bool> {
        ops::rules::sanitize_rules::delete(&self.conn, id).await
    }

    async fn list_cache_breakpoints(
        &self,
        provider_id: i64,
    ) -> anyhow::Result<Vec<CacheBreakpoint>> {
        ops::rules::cache_breakpoints::list(&self.conn, provider_id).await
    }

    async fn get_cache_breakpoint(&self, id: i64) -> anyhow::Result<Option<CacheBreakpoint>> {
        ops::rules::cache_breakpoints::get(&self.conn, id).await
    }

    async fn upsert_cache_breakpoint(
        &self,
        input: CacheBreakpointInput,
    ) -> anyhow::Result<CacheBreakpoint> {
        ops::rules::cache_breakpoints::upsert(&self.conn, input).await
    }

    async fn delete_cache_breakpoint(&self, id: i64) -> anyhow::Result<bool> {
        ops::rules::cache_breakpoints::delete(&self.conn, id).await
    }

    async fn list_beta_headers(&self, provider_id: i64) -> anyhow::Result<Vec<BetaHeader>> {
        ops::rules::beta_headers::list(&self.conn, provider_id).await
    }

    async fn get_beta_header(&self, id: i64) -> anyhow::Result<Option<BetaHeader>> {
        ops::rules::beta_headers::get(&self.conn, id).await
    }

    async fn upsert_beta_header(&self, input: BetaHeaderInput) -> anyhow::Result<BetaHeader> {
        ops::rules::beta_headers::upsert(&self.conn, input).await
    }

    async fn delete_beta_header(&self, id: i64) -> anyhow::Result<bool> {
        ops::rules::beta_headers::delete(&self.conn, id).await
    }

    async fn list_preludes_system(&self, provider_id: i64) -> anyhow::Result<Vec<PreludeSystem>> {
        ops::rules::preludes_system::list(&self.conn, provider_id).await
    }

    async fn get_prelude_system(&self, id: i64) -> anyhow::Result<Option<PreludeSystem>> {
        ops::rules::preludes_system::get(&self.conn, id).await
    }

    async fn upsert_prelude_system(
        &self,
        input: PreludeSystemInput,
    ) -> anyhow::Result<PreludeSystem> {
        ops::rules::preludes_system::upsert(&self.conn, input).await
    }

    async fn delete_prelude_system(&self, id: i64) -> anyhow::Result<bool> {
        ops::rules::preludes_system::delete(&self.conn, id).await
    }

    async fn list_orgs(&self) -> anyhow::Result<Vec<Org>> {
        ops::identity::orgs::list(&self.conn).await
    }

    async fn get_org(&self, id: i64) -> anyhow::Result<Option<Org>> {
        ops::identity::orgs::get(&self.conn, id).await
    }

    async fn get_org_by_name(&self, name: &str) -> anyhow::Result<Option<Org>> {
        ops::identity::orgs::get_by_name(&self.conn, name).await
    }

    async fn upsert_org(&self, input: OrgInput) -> anyhow::Result<Org> {
        ops::identity::orgs::upsert(&self.conn, input).await
    }

    async fn delete_org(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::orgs::delete(&self.conn, id).await
    }

    async fn list_teams(&self, org_id: i64) -> anyhow::Result<Vec<Team>> {
        ops::identity::teams::list(&self.conn, org_id).await
    }

    async fn get_team(&self, id: i64) -> anyhow::Result<Option<Team>> {
        ops::identity::teams::get(&self.conn, id).await
    }

    async fn upsert_team(&self, input: TeamInput) -> anyhow::Result<Team> {
        ops::identity::teams::upsert(&self.conn, input).await
    }

    async fn delete_team(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::teams::delete(&self.conn, id).await
    }

    async fn list_users(&self) -> anyhow::Result<Vec<User>> {
        ops::identity::users::list(&self.conn).await
    }

    async fn get_user(&self, id: i64) -> anyhow::Result<Option<User>> {
        ops::identity::users::get(&self.conn, id).await
    }

    async fn get_user_by_name(&self, name: &str) -> anyhow::Result<Option<User>> {
        ops::identity::users::get_by_name(&self.conn, name).await
    }

    async fn upsert_user(&self, input: UserInput) -> anyhow::Result<User> {
        ops::identity::users::upsert(&self.conn, input).await
    }

    async fn delete_user(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::users::delete(&self.conn, id).await
    }

    async fn list_user_keys(&self, user_id: i64) -> anyhow::Result<Vec<UserKey>> {
        ops::identity::user_keys::list(&self.conn, user_id).await
    }

    async fn get_user_key(&self, id: i64) -> anyhow::Result<Option<UserKey>> {
        ops::identity::user_keys::get(&self.conn, id).await
    }

    async fn find_user_key_by_digest(&self, digest: &str) -> anyhow::Result<Option<UserKey>> {
        ops::identity::user_keys::find_by_digest(&self.conn, digest).await
    }

    async fn upsert_user_key(&self, input: UserKeyInput) -> anyhow::Result<UserKey> {
        ops::identity::user_keys::upsert(&self.conn, input).await
    }

    async fn delete_user_key(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::user_keys::delete(&self.conn, id).await
    }

    async fn list_route_permissions(
        &self,
        scope: &str,
        scope_id: i64,
    ) -> anyhow::Result<Vec<RoutePermission>> {
        ops::identity::route_permissions::list(&self.conn, scope, scope_id).await
    }

    async fn upsert_route_permission(
        &self,
        input: RoutePermissionInput,
    ) -> anyhow::Result<RoutePermission> {
        ops::identity::route_permissions::upsert(&self.conn, input).await
    }

    async fn delete_route_permission(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::route_permissions::delete(&self.conn, id).await
    }

    async fn list_rate_limits(&self, scope: &str, scope_id: i64) -> anyhow::Result<Vec<RateLimit>> {
        ops::identity::rate_limits::list(&self.conn, scope, scope_id).await
    }

    async fn upsert_rate_limit(&self, input: RateLimitInput) -> anyhow::Result<RateLimit> {
        ops::identity::rate_limits::upsert(&self.conn, input).await
    }

    async fn delete_rate_limit(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::rate_limits::delete(&self.conn, id).await
    }

    async fn get_quota(&self, scope: &str, scope_id: i64) -> anyhow::Result<Option<Quota>> {
        ops::identity::quotas::get(&self.conn, scope, scope_id).await
    }

    async fn upsert_quota(&self, input: QuotaInput) -> anyhow::Result<Quota> {
        ops::identity::quotas::upsert(&self.conn, input).await
    }

    async fn delete_quota(&self, id: i64) -> anyhow::Result<bool> {
        ops::identity::quotas::delete(&self.conn, id).await
    }

    async fn append_usage(&self, input: UsageInput) -> anyhow::Result<Usage> {
        ops::usage::usages::append(&self.conn, input).await
    }

    async fn list_usages(&self, limit: u64) -> anyhow::Result<Vec<Usage>> {
        ops::usage::usages::list(&self.conn, limit).await
    }

    async fn add_usage_rollup(&self, input: UsageRollupInput) -> anyhow::Result<UsageRollup> {
        ops::usage::usage_rollups::add(&self.conn, input).await
    }

    async fn list_usage_rollups(
        &self,
        granularity: &str,
        from: i64,
        to: i64,
    ) -> anyhow::Result<Vec<UsageRollup>> {
        ops::usage::usage_rollups::list(&self.conn, granularity, from, to).await
    }

    async fn append_downstream_request(
        &self,
        input: DownstreamRequestInput,
    ) -> anyhow::Result<DownstreamRequest> {
        ops::usage::downstream_requests::append(&self.conn, input).await
    }

    async fn list_downstream_requests(
        &self,
        request_id: &str,
    ) -> anyhow::Result<Vec<DownstreamRequest>> {
        ops::usage::downstream_requests::list(&self.conn, request_id).await
    }

    async fn append_upstream_request(
        &self,
        input: UpstreamRequestInput,
    ) -> anyhow::Result<UpstreamRequest> {
        ops::usage::upstream_requests::append(&self.conn, input).await
    }

    async fn list_upstream_requests(
        &self,
        request_id: &str,
    ) -> anyhow::Result<Vec<UpstreamRequest>> {
        ops::usage::upstream_requests::list(&self.conn, request_id).await
    }

    async fn list_instance_settings(&self) -> anyhow::Result<Vec<InstanceSettings>> {
        ops::settings::instance_settings::list(&self.conn).await
    }

    async fn get_instance_settings(
        &self,
        instance_name: &str,
    ) -> anyhow::Result<Option<InstanceSettings>> {
        ops::settings::instance_settings::get(&self.conn, instance_name).await
    }

    async fn upsert_instance_settings(
        &self,
        input: InstanceSettingsInput,
    ) -> anyhow::Result<InstanceSettings> {
        ops::settings::instance_settings::upsert(&self.conn, input).await
    }
}

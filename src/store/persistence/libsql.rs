//! Edge (wasm32) persistence backend backed by libSQL/Turso over Hrana HTTP.
//!
//! `LibsqlPersistence` wraps [`LibsqlClient`] and implements
//! [`PersistenceBackend`]. `health()` executes `SELECT 1`.
//!
//! Compile-checked on wasm32; round-trips require Turso credentials
//! (see ignored integration test).

use crate::store::libsql::LibsqlClient;

use super::PersistenceBackend;
use super::records::{
    Alias, AliasInput, Credential, CredentialInput, CredentialStatus, CredentialStatusInput,
    DownstreamRequest, DownstreamRequestInput, InstanceSettings, InstanceSettingsInput, Org,
    OrgInput, Provider, ProviderInput, ProviderModel, ProviderModelInput, ProviderRuleSet,
    ProviderRuleSetInput, Quota, QuotaInput, RateLimit, RateLimitInput, Route, RouteInput,
    RouteMember, RouteMemberInput, RoutePermission, RoutePermissionInput, RoutingRule,
    RoutingRuleInput, Rule, RuleInput, RuleSet, RuleSetInput, Scope, Team, TeamInput,
    UpstreamRequest, UpstreamRequestInput, Usage, UsageInput, UsageRollup, UsageRollupInput, User,
    UserInput, UserKey, UserKeyInput,
};

/// Edge persistence backend backed by a Turso/libSQL database via Hrana HTTP.
pub struct LibsqlPersistence {
    client: LibsqlClient,
}

impl LibsqlPersistence {
    /// Create a new persistence backend.
    ///
    /// `url` — Turso database URL (e.g. `https://<db>.turso.io`).
    /// `token` — Bearer auth token.
    pub fn connect(url: String, token: String) -> Self {
        Self {
            client: LibsqlClient::new(url, token),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl PersistenceBackend for LibsqlPersistence {
    fn kind(&self) -> &'static str {
        "libsql"
    }

    async fn health(&self) -> anyhow::Result<()> {
        self.client
            .execute("SELECT 1", &[])
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("libsql health failed: {e}"))
    }

    // TODO(edge): implement entity ops as hand-written SQLite SQL over the Hrana
    // client. Deferred to the edge AppState/entry phase (§10.8); the trait stays
    // coherent for the wasm build via these stubs.
    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn get_provider(&self, _id: i64) -> anyhow::Result<Option<Provider>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn get_provider_by_name(&self, _name: &str) -> anyhow::Result<Option<Provider>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn upsert_provider(&self, _input: ProviderInput) -> anyhow::Result<Provider> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn delete_provider(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn list_credentials(&self, _provider_id: i64) -> anyhow::Result<Vec<Credential>> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn get_credential(&self, _id: i64) -> anyhow::Result<Option<Credential>> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn upsert_credential(&self, _input: CredentialInput) -> anyhow::Result<Credential> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn delete_credential(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn list_credential_statuses(
        &self,
        _credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn upsert_credential_status(
        &self,
        _input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn delete_credential_status(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn get_route(&self, _id: i64) -> anyhow::Result<Option<Route>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn get_route_by_name(&self, _name: &str) -> anyhow::Result<Option<Route>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn upsert_route(&self, _input: RouteInput) -> anyhow::Result<Route> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn delete_route(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn list_route_members(&self, _route_id: i64) -> anyhow::Result<Vec<RouteMember>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn upsert_route_member(&self, _input: RouteMemberInput) -> anyhow::Result<RouteMember> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn delete_route_member(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn get_alias_by_name(&self, _alias: &str) -> anyhow::Result<Option<Alias>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn upsert_alias(&self, _input: AliasInput) -> anyhow::Result<Alias> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn delete_alias(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn list_provider_models(&self, _provider_id: i64) -> anyhow::Result<Vec<ProviderModel>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn upsert_provider_model(
        &self,
        _input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn delete_provider_model(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn list_routing_rules(&self, _provider_id: i64) -> anyhow::Result<Vec<RoutingRule>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn get_routing_rule(&self, _id: i64) -> anyhow::Result<Option<RoutingRule>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn upsert_routing_rule(&self, _input: RoutingRuleInput) -> anyhow::Result<RoutingRule> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn delete_routing_rule(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn list_rule_sets(&self) -> anyhow::Result<Vec<RuleSet>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn get_rule_set(&self, _id: i64) -> anyhow::Result<Option<RuleSet>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn get_rule_set_by_name(&self, _name: &str) -> anyhow::Result<Option<RuleSet>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn upsert_rule_set(&self, _input: RuleSetInput) -> anyhow::Result<RuleSet> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn delete_rule_set(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn list_rules(&self, _rule_set_id: i64) -> anyhow::Result<Vec<Rule>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn get_rule(&self, _id: i64) -> anyhow::Result<Option<Rule>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn upsert_rule(&self, _input: RuleInput) -> anyhow::Result<Rule> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn delete_rule(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn list_provider_rule_sets(
        &self,
        _provider_id: i64,
    ) -> anyhow::Result<Vec<ProviderRuleSet>> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn upsert_provider_rule_set(
        &self,
        _input: ProviderRuleSetInput,
    ) -> anyhow::Result<ProviderRuleSet> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn delete_provider_rule_set(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql rules ops not implemented yet")
    }

    async fn list_orgs(&self) -> anyhow::Result<Vec<Org>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_org(&self, _id: i64) -> anyhow::Result<Option<Org>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_org_by_name(&self, _name: &str) -> anyhow::Result<Option<Org>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_org(&self, _input: OrgInput) -> anyhow::Result<Org> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_org(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn list_teams(&self, _org_id: i64) -> anyhow::Result<Vec<Team>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_team(&self, _id: i64) -> anyhow::Result<Option<Team>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_team(&self, _input: TeamInput) -> anyhow::Result<Team> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_team(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn list_users(&self) -> anyhow::Result<Vec<User>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_user(&self, _id: i64) -> anyhow::Result<Option<User>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_user_by_name(&self, _name: &str) -> anyhow::Result<Option<User>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_user(&self, _input: UserInput) -> anyhow::Result<User> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_user(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn list_user_keys(&self, _user_id: i64) -> anyhow::Result<Vec<UserKey>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_user_key(&self, _id: i64) -> anyhow::Result<Option<UserKey>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn find_user_key_by_digest(&self, _digest: &str) -> anyhow::Result<Option<UserKey>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_user_key(&self, _input: UserKeyInput) -> anyhow::Result<UserKey> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_user_key(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn list_route_permissions(
        &self,
        _scope: Scope,
        _scope_id: i64,
    ) -> anyhow::Result<Vec<RoutePermission>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_route_permission(
        &self,
        _input: RoutePermissionInput,
    ) -> anyhow::Result<RoutePermission> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_route_permission(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn list_rate_limits(
        &self,
        _scope: Scope,
        _scope_id: i64,
    ) -> anyhow::Result<Vec<RateLimit>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_rate_limit(&self, _input: RateLimitInput) -> anyhow::Result<RateLimit> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_rate_limit(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn get_quota(&self, _scope: Scope, _scope_id: i64) -> anyhow::Result<Option<Quota>> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn upsert_quota(&self, _input: QuotaInput) -> anyhow::Result<Quota> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn delete_quota(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn add_quota_cost(
        &self,
        _scope: Scope,
        _scope_id: i64,
        _delta: rust_decimal::Decimal,
    ) -> anyhow::Result<()> {
        anyhow::bail!("libsql identity ops not implemented yet")
    }

    async fn append_usage(&self, _input: UsageInput) -> anyhow::Result<Option<Usage>> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn list_usages(&self, _limit: u64) -> anyhow::Result<Vec<Usage>> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn add_usage_rollup(&self, _input: UsageRollupInput) -> anyhow::Result<UsageRollup> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn list_usage_rollups(
        &self,
        _granularity: &str,
        _from: i64,
        _to: i64,
    ) -> anyhow::Result<Vec<UsageRollup>> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn append_downstream_request(
        &self,
        _input: DownstreamRequestInput,
    ) -> anyhow::Result<DownstreamRequest> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn list_downstream_requests(
        &self,
        _request_id: &str,
    ) -> anyhow::Result<Vec<DownstreamRequest>> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn append_upstream_request(
        &self,
        _input: UpstreamRequestInput,
    ) -> anyhow::Result<UpstreamRequest> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn list_upstream_requests(
        &self,
        _request_id: &str,
    ) -> anyhow::Result<Vec<UpstreamRequest>> {
        anyhow::bail!("libsql usage ops not implemented yet")
    }

    async fn list_instance_settings(&self) -> anyhow::Result<Vec<InstanceSettings>> {
        anyhow::bail!("libsql instance settings ops not implemented yet")
    }

    async fn get_instance_settings(
        &self,
        _instance_name: &str,
    ) -> anyhow::Result<Option<InstanceSettings>> {
        anyhow::bail!("libsql instance settings ops not implemented yet")
    }

    async fn upsert_instance_settings(
        &self,
        _input: InstanceSettingsInput,
    ) -> anyhow::Result<InstanceSettings> {
        anyhow::bail!("libsql instance settings ops not implemented yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[wasm_bindgen_test::wasm_bindgen_test]
    #[ignore = "requires live Turso creds via GPROXY_TEST_TURSO_URL / GPROXY_TEST_TURSO_TOKEN"]
    async fn integration_health() {
        let url = std::env::var("GPROXY_TEST_TURSO_URL").expect("GPROXY_TEST_TURSO_URL");
        let token = std::env::var("GPROXY_TEST_TURSO_TOKEN").expect("GPROXY_TEST_TURSO_TOKEN");
        let backend = LibsqlPersistence::connect(url, token);
        backend.health().await.expect("health");
    }
}

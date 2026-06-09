//! The control-plane snapshot (§7.2): the sole `ArcSwap` snapshot read on the
//! hot path. Fully rebuildable from persistence (boot + invalidation). Holds no
//! counters/sessions/health — those are redis-direct or separate local state.
//!
//! M2/M3 extend THIS struct + [`ControlPlaneSnapshot::build`], never a parallel
//! snapshot.

use std::collections::HashMap;
use std::sync::Arc;

use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::{
    Credential, Provider, ProviderModel, Route, RouteMember, User, UserKey,
};

/// Immutable control-plane snapshot.
pub struct ControlPlaneSnapshot {
    pub providers_by_name: HashMap<String, Arc<Provider>>,
    pub providers_by_id: HashMap<i64, Arc<Provider>>,
    pub routes_by_name: HashMap<String, Arc<ResolvedRoute>>,
    /// alias name → canonical route name (many-to-one).
    pub alias_to_route: HashMap<String, String>,
    /// api-key digest → identity (auth without a DB hit). ENABLED keys + users.
    pub keys_by_digest: HashMap<String, Arc<KeyIdentity>>,
    /// provider id → ENABLED credential pool.
    pub credentials_by_provider: HashMap<i64, Vec<Arc<Credential>>>,
    /// provider id → models.
    pub models_by_provider: HashMap<i64, Vec<Arc<ProviderModel>>>,
    /// Bumped on each rebuild.
    pub version: u64,
}

/// A route plus its members, pre-sorted by `(tier asc, weight desc)`.
pub struct ResolvedRoute {
    pub route: Route,
    pub members: Vec<RouteMember>,
}

/// Auth identity resolved from a user key (`org_id`/`team_id` used by M3 authz).
pub struct KeyIdentity {
    pub user_key: UserKey,
    pub user: User,
}

impl ControlPlaneSnapshot {
    /// An empty snapshot (used transiently at boot before the first build).
    pub fn empty(version: u64) -> Self {
        Self {
            providers_by_name: HashMap::new(),
            providers_by_id: HashMap::new(),
            routes_by_name: HashMap::new(),
            alias_to_route: HashMap::new(),
            keys_by_digest: HashMap::new(),
            credentials_by_provider: HashMap::new(),
            models_by_provider: HashMap::new(),
            version,
        }
    }

    /// Full reload from persistence (boot + invalidation). On wasm the backend
    /// trait is `?Send`, so this future is non-Send — await it directly, never
    /// on a `Send`-requiring spawn.
    pub async fn build(db: &dyn PersistenceBackend, version: u64) -> anyhow::Result<Self> {
        let mut snap = Self::empty(version);

        // providers + their credentials/models
        for provider in db.list_providers().await? {
            let pid = provider.id;
            let creds = db
                .list_credentials(pid)
                .await?
                .into_iter()
                .filter(|c| c.enabled)
                .map(Arc::new)
                .collect::<Vec<_>>();
            let models = db
                .list_provider_models(pid)
                .await?
                .into_iter()
                .map(Arc::new)
                .collect::<Vec<_>>();
            snap.credentials_by_provider.insert(pid, creds);
            snap.models_by_provider.insert(pid, models);

            let provider = Arc::new(provider);
            snap.providers_by_name
                .insert(provider.name.clone(), Arc::clone(&provider));
            snap.providers_by_id.insert(pid, provider);
        }

        // routes + members (sorted) + a route-id → name map for aliases
        let mut route_name_by_id: HashMap<i64, String> = HashMap::new();
        for route in db.list_routes().await? {
            let mut members = db.list_route_members(route.id).await?;
            members.retain(|m| m.enabled);
            members.sort_by(|a, b| a.tier.cmp(&b.tier).then(b.weight.cmp(&a.weight)));
            route_name_by_id.insert(route.id, route.name.clone());
            let name = route.name.clone();
            snap.routes_by_name
                .insert(name, Arc::new(ResolvedRoute { route, members }));
        }

        // aliases → route name
        for alias in db.list_aliases().await? {
            if let Some(name) = route_name_by_id.get(&alias.route_id) {
                snap.alias_to_route.insert(alias.alias, name.clone());
            }
        }

        // users (enabled) + their keys (enabled), indexed by digest
        for user in db.list_users().await?.into_iter().filter(|u| u.enabled) {
            let keys = db.list_user_keys(user.id).await?;
            let user = Arc::new(user);
            for key in keys.into_iter().filter(|k| k.enabled) {
                let digest = key.api_key_digest.clone();
                let identity = Arc::new(KeyIdentity {
                    user_key: key,
                    user: User::clone(&user),
                });
                snap.keys_by_digest.insert(digest, identity);
            }
        }

        Ok(snap)
    }
}

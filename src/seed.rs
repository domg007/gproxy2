//! First-run bootstrap seeding (native; edge does not seed). Backend-agnostic:
//! uses the `upsert_*` trait methods, gated by an emptiness check so it is
//! idempotent. Produces a self-contained smoke setup: one org/team, a mock
//! OpenAI-compatible provider + credential + model, a route+member+alias, and a
//! user + API key.

use serde_json::json;

use crate::pipeline::auth::key_digest;
use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::{
    AliasInput, CredentialInput, OrgInput, ProviderInput, ProviderModelInput, RouteInput,
    RouteMemberInput, TeamInput, UserInput, UserKeyInput,
};

/// The bare API key the seeded user key authenticates with.
pub const SEED_API_KEY: &str = "sk-smoke-123";

/// Seed a minimal working configuration if none exists yet. Idempotent: returns
/// immediately if any provider is already present.
pub async fn seed_if_empty(db: &dyn PersistenceBackend) -> anyhow::Result<()> {
    if !db.list_providers().await?.is_empty() {
        return Ok(());
    }

    // identity (org/team first so the user FK resolves on the db backend)
    let org = db
        .upsert_org(OrgInput {
            id: None,
            name: "default".into(),
            enabled: true,
            description: None,
        })
        .await?;
    let team = db
        .upsert_team(TeamInput {
            id: None,
            org_id: org.id,
            name: "default".into(),
            enabled: true,
        })
        .await?;

    // provider + credential + model
    let provider = db
        .upsert_provider(ProviderInput {
            id: None,
            name: "mock-openai".into(),
            channel: "openai_compatible".into(),
            label: None,
            settings_json: json!({ "base_url": "http://127.0.0.1:9009" }),
            credential_strategy: "round_robin".into(),
            tls_fingerprint: None,
            enabled: true,
        })
        .await?;
    db.upsert_credential(CredentialInput {
        id: None,
        provider_id: provider.id,
        name: None,
        kind: "api_key".into(),
        secret_json: json!({ "api_key": "sk-mock" }),
        weight: 1,
        rpm_limit: None,
        tpm_limit: None,
        proxy_url: None,
        enabled: true,
    })
    .await?;
    db.upsert_provider_model(ProviderModelInput {
        id: None,
        provider_id: provider.id,
        model_id: "gpt-4o-mini".into(),
        display_name: None,
        pricing_json: None,
        enabled: true,
    })
    .await?;

    // route + member + alias
    let route = db
        .upsert_route(RouteInput {
            id: None,
            name: "gpt-4o-mini".into(),
            strategy: "weighted".into(),
            enabled: true,
            description: None,
        })
        .await?;
    db.upsert_route_member(RouteMemberInput {
        id: None,
        route_id: route.id,
        provider_id: provider.id,
        upstream_model_id: "gpt-4o-mini".into(),
        weight: 1,
        tier: 0,
        enabled: true,
    })
    .await?;
    db.upsert_alias(AliasInput {
        id: None,
        alias: "gpt-4o-mini".into(),
        route_id: route.id,
    })
    .await?;

    // user + key
    let user = db
        .upsert_user(UserInput {
            id: None,
            name: "smoke".into(),
            org_id: org.id,
            team_id: Some(team.id),
            password: None,
            enabled: true,
            is_admin: false,
        })
        .await?;
    db.upsert_user_key(UserKeyInput {
        id: None,
        user_id: user.id,
        api_key_ciphertext: format!("plain:{SEED_API_KEY}"),
        api_key_digest: key_digest(SEED_API_KEY),
        label: None,
        enabled: true,
    })
    .await?;

    Ok(())
}

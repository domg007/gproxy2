//! Tests for [`DbPersistence`].

use super::DbPersistence;
use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::*;
use serde_json::json;

async fn mem() -> DbPersistence {
    DbPersistence::connect("sqlite::memory:")
        .await
        .expect("connect")
}

#[tokio::test]
async fn sqlite_memory_connect_and_health() {
    mem().await.health().await.expect("health");
}

#[tokio::test]
async fn provider_round_trip() {
    let db = mem().await;
    let created = db
        .upsert_provider(ProviderInput {
            id: None,
            name: "openai".to_owned(),
            channel: "openai".to_owned(),
            label: Some("OpenAI".to_owned()),
            settings_json: json!({"base_url": "https://api.openai.com"}),
            credential_strategy: "round_robin".to_owned(),
            proxy_url: None,
            tls_fingerprint: None,
            enabled: true,
        })
        .await
        .expect("insert");
    assert!(created.id > 0);

    let fetched = db
        .get_provider_by_name("openai")
        .await
        .expect("get")
        .expect("some");
    assert_eq!(fetched, created);

    let updated = db
        .upsert_provider(ProviderInput {
            id: Some(created.id),
            name: "openai".to_owned(),
            channel: "openai".to_owned(),
            label: None,
            settings_json: json!({"base_url": "https://x"}),
            credential_strategy: "sticky".to_owned(),
            proxy_url: None,
            tls_fingerprint: None,
            enabled: false,
        })
        .await
        .expect("update");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.credential_strategy, "sticky");
    assert!(!updated.enabled);

    assert_eq!(db.list_providers().await.expect("list").len(), 1);
    assert!(db.delete_provider(created.id).await.expect("delete"));
    assert!(db.list_providers().await.expect("list").is_empty());
}

#[tokio::test]
async fn cascade_deletes() {
    let db = mem().await;

    // provider → credential → status, and provider → model.
    let p = db
        .upsert_provider(ProviderInput {
            id: None,
            name: "p".to_owned(),
            channel: "openai".to_owned(),
            label: None,
            settings_json: json!({}),
            credential_strategy: "round_robin".to_owned(),
            proxy_url: None,
            tls_fingerprint: None,
            enabled: true,
        })
        .await
        .unwrap();
    let c = db
        .upsert_credential(CredentialInput {
            id: None,
            provider_id: p.id,
            name: None,
            kind: "api_key".to_owned(),
            secret_json: json!({"key": "x"}),
            weight: 1,
            rpm_limit: None,
            tpm_limit: None,
            proxy_url: None,
            tls_fingerprint: None,
            enabled: true,
        })
        .await
        .unwrap();
    db.upsert_credential_status(CredentialStatusInput {
        id: None,
        credential_id: c.id,
        channel: "openai".to_owned(),
        health_kind: "ok".to_owned(),
        health_json: None,
        checked_at: None,
        last_error: None,
    })
    .await
    .unwrap();
    db.upsert_provider_model(ProviderModelInput {
        id: None,
        provider_id: p.id,
        model_id: "gpt-x".to_owned(),
        display_name: None,
        pricing_json: None,
        enabled: true,
    })
    .await
    .unwrap();

    db.delete_provider(p.id).await.unwrap();
    assert!(db.list_credentials(p.id).await.unwrap().is_empty());
    assert!(db.list_credential_statuses(c.id).await.unwrap().is_empty());
    assert!(db.list_provider_models(p.id).await.unwrap().is_empty());

    // route → member + alias.
    let r = db
        .upsert_route(RouteInput {
            id: None,
            name: "r".to_owned(),
            strategy: "weighted".to_owned(),
            enabled: true,
            description: None,
        })
        .await
        .unwrap();
    db.upsert_route_member(RouteMemberInput {
        id: None,
        route_id: r.id,
        provider_id: p.id,
        upstream_model_id: "gpt-x".to_owned(),
        weight: 1,
        tier: 0,
        enabled: true,
    })
    .await
    .unwrap();
    db.upsert_alias(AliasInput {
        id: None,
        alias: "a".to_owned(),
        route_id: r.id,
    })
    .await
    .unwrap();

    db.delete_route(r.id).await.unwrap();
    assert!(db.list_route_members(r.id).await.unwrap().is_empty());
    assert!(db.get_alias_by_name("a").await.unwrap().is_none());
}

#[tokio::test]
async fn quota_decimal_exact_round_trip() {
    use rust_decimal::Decimal;

    // Money is stored as a decimal STRING; assert it survives the sqlite
    // round-trip with no float drift (exact equality, not approximate).
    let db = mem().await;
    let quota_total = "0.000000123".parse::<Decimal>().unwrap();
    let cost_used = "0.000000001".parse::<Decimal>().unwrap();

    let created = db
        .upsert_quota(QuotaInput {
            id: None,
            scope: Scope::User,
            scope_id: 42,
            quota_total,
            cost_used,
        })
        .await
        .expect("upsert quota");

    let fetched = db
        .get_quota(Scope::User, 42)
        .await
        .expect("get quota")
        .expect("quota present");

    assert_eq!(fetched.quota_total, quota_total);
    assert_eq!(fetched.cost_used, cost_used);
    assert_eq!(fetched, created);
}

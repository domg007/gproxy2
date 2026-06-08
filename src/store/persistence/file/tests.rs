//! Tests for [`FilePersistence`].

use super::FilePersistence;
use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::*;
use serde_json::json;

async fn open() -> (tempfile::TempDir, FilePersistence) {
    let dir = tempfile::tempdir().expect("tempdir");
    let fp = FilePersistence::open(dir.path().to_path_buf())
        .await
        .expect("open");
    (dir, fp)
}

#[tokio::test]
async fn open_and_health_ok() {
    let (_dir, fp) = open().await;
    fp.health().await.expect("health");
}

#[tokio::test]
async fn provider_round_trip() {
    let (_dir, fp) = open().await;
    let input = ProviderInput {
        id: None,
        name: "openai".to_owned(),
        channel: "openai".to_owned(),
        label: Some("OpenAI".to_owned()),
        settings_json: json!({"base_url": "https://api.openai.com"}),
        credential_strategy: "round_robin".to_owned(),
        tls_fingerprint: None,
        enabled: true,
    };
    let created = fp.upsert_provider(input).await.expect("insert");
    assert!(created.id > 0);

    // Duplicate name rejected.
    assert!(
        fp.upsert_provider(ProviderInput {
            id: None,
            name: "openai".to_owned(),
            channel: "x".to_owned(),
            label: None,
            settings_json: json!({}),
            credential_strategy: "round_robin".to_owned(),
            tls_fingerprint: None,
            enabled: true,
        })
        .await
        .is_err()
    );

    assert_eq!(
        fp.get_provider_by_name("openai")
            .await
            .expect("get")
            .as_ref(),
        Some(&created)
    );

    let updated = fp
        .upsert_provider(ProviderInput {
            id: Some(created.id),
            name: "openai".to_owned(),
            channel: "openai".to_owned(),
            label: None,
            settings_json: json!({"base_url": "https://x"}),
            credential_strategy: "sticky".to_owned(),
            tls_fingerprint: None,
            enabled: false,
        })
        .await
        .expect("update");
    assert_eq!(updated.credential_strategy, "sticky");

    assert!(fp.delete_provider(created.id).await.expect("delete"));
    assert!(fp.list_providers().await.expect("list").is_empty());
}

//! Database persistence backend (SeaORM).

use async_trait::async_trait;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

use super::PersistenceBackend;
use super::records::{
    Alias, AliasInput, Credential, CredentialInput, CredentialStatus, CredentialStatusInput,
    Provider, ProviderInput, ProviderModel, ProviderModelInput, Route, RouteInput, RouteMember,
    RouteMemberInput,
};

mod entities;
mod ops;
mod schema;

/// Persistence backend backed by a SeaORM-managed database.
///
/// Supports SQLite, PostgreSQL, and MySQL. Connect with
/// [`connect`](DbPersistence::connect), passing a standard connection DSN.
/// SeaORM is an internal detail of this backend; domain code uses only the
/// [`PersistenceBackend`] trait.
pub struct DbPersistence {
    conn: DatabaseConnection,
}

impl DbPersistence {
    /// Connect to the database identified by `dsn` and ensure the schema exists.
    pub async fn connect(dsn: &str) -> anyhow::Result<Self> {
        let opts = ConnectOptions::new(dsn.to_string());
        let conn = Database::connect(opts)
            .await
            .map_err(|e| anyhow::anyhow!("db connect failed: {e}"))?;
        schema::create_all(&conn).await?;
        Ok(Self { conn })
    }
}

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
        ops::providers::list(&self.conn).await
    }

    async fn get_provider(&self, id: i64) -> anyhow::Result<Option<Provider>> {
        ops::providers::get(&self.conn, id).await
    }

    async fn get_provider_by_name(&self, name: &str) -> anyhow::Result<Option<Provider>> {
        ops::providers::get_by_name(&self.conn, name).await
    }

    async fn upsert_provider(&self, input: ProviderInput) -> anyhow::Result<Provider> {
        ops::providers::upsert(&self.conn, input).await
    }

    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool> {
        ops::providers::delete(&self.conn, id).await
    }

    async fn list_credentials(&self, provider_id: i64) -> anyhow::Result<Vec<Credential>> {
        ops::credentials::list(&self.conn, provider_id).await
    }

    async fn get_credential(&self, id: i64) -> anyhow::Result<Option<Credential>> {
        ops::credentials::get(&self.conn, id).await
    }

    async fn upsert_credential(&self, input: CredentialInput) -> anyhow::Result<Credential> {
        ops::credentials::upsert(&self.conn, input).await
    }

    async fn delete_credential(&self, id: i64) -> anyhow::Result<bool> {
        ops::credentials::delete(&self.conn, id).await
    }

    async fn list_credential_statuses(
        &self,
        credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>> {
        ops::credential_statuses::list(&self.conn, credential_id).await
    }

    async fn upsert_credential_status(
        &self,
        input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus> {
        ops::credential_statuses::upsert(&self.conn, input).await
    }

    async fn delete_credential_status(&self, id: i64) -> anyhow::Result<bool> {
        ops::credential_statuses::delete(&self.conn, id).await
    }

    async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        ops::routes::list(&self.conn).await
    }

    async fn get_route(&self, id: i64) -> anyhow::Result<Option<Route>> {
        ops::routes::get(&self.conn, id).await
    }

    async fn get_route_by_name(&self, name: &str) -> anyhow::Result<Option<Route>> {
        ops::routes::get_by_name(&self.conn, name).await
    }

    async fn upsert_route(&self, input: RouteInput) -> anyhow::Result<Route> {
        ops::routes::upsert(&self.conn, input).await
    }

    async fn delete_route(&self, id: i64) -> anyhow::Result<bool> {
        ops::routes::delete(&self.conn, id).await
    }

    async fn list_route_members(&self, route_id: i64) -> anyhow::Result<Vec<RouteMember>> {
        ops::route_members::list(&self.conn, route_id).await
    }

    async fn upsert_route_member(&self, input: RouteMemberInput) -> anyhow::Result<RouteMember> {
        ops::route_members::upsert(&self.conn, input).await
    }

    async fn delete_route_member(&self, id: i64) -> anyhow::Result<bool> {
        ops::route_members::delete(&self.conn, id).await
    }

    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>> {
        ops::aliases::list(&self.conn).await
    }

    async fn get_alias_by_name(&self, alias: &str) -> anyhow::Result<Option<Alias>> {
        ops::aliases::get_by_name(&self.conn, alias).await
    }

    async fn upsert_alias(&self, input: AliasInput) -> anyhow::Result<Alias> {
        ops::aliases::upsert(&self.conn, input).await
    }

    async fn delete_alias(&self, id: i64) -> anyhow::Result<bool> {
        ops::aliases::delete(&self.conn, id).await
    }

    async fn list_provider_models(&self, provider_id: i64) -> anyhow::Result<Vec<ProviderModel>> {
        ops::provider_models::list(&self.conn, provider_id).await
    }

    async fn upsert_provider_model(
        &self,
        input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel> {
        ops::provider_models::upsert(&self.conn, input).await
    }

    async fn delete_provider_model(&self, id: i64) -> anyhow::Result<bool> {
        ops::provider_models::delete(&self.conn, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}

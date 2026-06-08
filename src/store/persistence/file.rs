//! File-system persistence backend.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::PersistenceBackend;
use super::records::{
    Alias, AliasInput, Credential, CredentialInput, CredentialStatus, CredentialStatusInput,
    Provider, ProviderInput, ProviderModel, ProviderModelInput, Route, RouteInput, RouteMember,
    RouteMemberInput,
};

mod aliases;
mod credential_statuses;
mod credentials;
mod provider_models;
mod providers;
mod route_members;
mod routes;
mod table;

/// Persistence backend backed by the local file system.
///
/// Suitable for single-instance deployments. The `root` directory is
/// created on [`open`](FilePersistence::open) if it does not exist. Each table
/// is one JSON file under `root`; the `write` mutex serializes mutations
/// (control-plane data is low-write, so whole-file read/rewrite is fine).
pub struct FilePersistence {
    root: PathBuf,
    write: Mutex<()>,
}

impl FilePersistence {
    /// Open (and create if absent) the data directory at `data_dir`.
    ///
    /// Only ensures the directory exists; write-permission is verified by
    /// [`health`](FilePersistence::health), which callers should invoke at startup.
    pub async fn open(data_dir: PathBuf) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&data_dir).await.map_err(|e| {
            anyhow::anyhow!("failed to create data dir {}: {e}", data_dir.display())
        })?;
        Ok(Self {
            root: data_dir,
            write: Mutex::new(()),
        })
    }
}

#[async_trait]
impl PersistenceBackend for FilePersistence {
    fn kind(&self) -> &'static str {
        "file"
    }

    async fn health(&self) -> anyhow::Result<()> {
        let probe = self
            .root
            .join(format!(".gproxy_health_{}", std::process::id()));
        tokio::fs::write(&probe, b"ok")
            .await
            .map_err(|e| anyhow::anyhow!("data dir is not writable: {e}"))?;
        tokio::fs::remove_file(&probe).await.ok();
        Ok(())
    }

    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        providers::list(&self.root).await
    }

    async fn get_provider(&self, id: i64) -> anyhow::Result<Option<Provider>> {
        providers::get(&self.root, id).await
    }

    async fn get_provider_by_name(&self, name: &str) -> anyhow::Result<Option<Provider>> {
        providers::get_by_name(&self.root, name).await
    }

    async fn upsert_provider(&self, input: ProviderInput) -> anyhow::Result<Provider> {
        let _guard = self.write.lock().await;
        providers::upsert(&self.root, input).await
    }

    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        providers::delete(&self.root, id).await
    }

    async fn list_credentials(&self, provider_id: i64) -> anyhow::Result<Vec<Credential>> {
        credentials::list(&self.root, provider_id).await
    }

    async fn get_credential(&self, id: i64) -> anyhow::Result<Option<Credential>> {
        credentials::get(&self.root, id).await
    }

    async fn upsert_credential(&self, input: CredentialInput) -> anyhow::Result<Credential> {
        let _guard = self.write.lock().await;
        credentials::upsert(&self.root, input).await
    }

    async fn delete_credential(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        credentials::delete(&self.root, id).await
    }

    async fn list_credential_statuses(
        &self,
        credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>> {
        credential_statuses::list(&self.root, credential_id).await
    }

    async fn upsert_credential_status(
        &self,
        input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus> {
        let _guard = self.write.lock().await;
        credential_statuses::upsert(&self.root, input).await
    }

    async fn delete_credential_status(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        credential_statuses::delete(&self.root, id).await
    }

    async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        routes::list(&self.root).await
    }

    async fn get_route(&self, id: i64) -> anyhow::Result<Option<Route>> {
        routes::get(&self.root, id).await
    }

    async fn get_route_by_name(&self, name: &str) -> anyhow::Result<Option<Route>> {
        routes::get_by_name(&self.root, name).await
    }

    async fn upsert_route(&self, input: RouteInput) -> anyhow::Result<Route> {
        let _guard = self.write.lock().await;
        routes::upsert(&self.root, input).await
    }

    async fn delete_route(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        routes::delete(&self.root, id).await
    }

    async fn list_route_members(&self, route_id: i64) -> anyhow::Result<Vec<RouteMember>> {
        route_members::list(&self.root, route_id).await
    }

    async fn upsert_route_member(&self, input: RouteMemberInput) -> anyhow::Result<RouteMember> {
        let _guard = self.write.lock().await;
        route_members::upsert(&self.root, input).await
    }

    async fn delete_route_member(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        route_members::delete(&self.root, id).await
    }

    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>> {
        aliases::list(&self.root).await
    }

    async fn get_alias_by_name(&self, alias: &str) -> anyhow::Result<Option<Alias>> {
        aliases::get_by_name(&self.root, alias).await
    }

    async fn upsert_alias(&self, input: AliasInput) -> anyhow::Result<Alias> {
        let _guard = self.write.lock().await;
        aliases::upsert(&self.root, input).await
    }

    async fn delete_alias(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        aliases::delete(&self.root, id).await
    }

    async fn list_provider_models(&self, provider_id: i64) -> anyhow::Result<Vec<ProviderModel>> {
        provider_models::list(&self.root, provider_id).await
    }

    async fn upsert_provider_model(
        &self,
        input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel> {
        let _guard = self.write.lock().await;
        provider_models::upsert(&self.root, input).await
    }

    async fn delete_provider_model(&self, id: i64) -> anyhow::Result<bool> {
        let _guard = self.write.lock().await;
        provider_models::delete(&self.root, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                enabled: false,
            })
            .await
            .expect("update");
        assert_eq!(updated.credential_strategy, "sticky");

        assert!(fp.delete_provider(created.id).await.expect("delete"));
        assert!(fp.list_providers().await.expect("list").is_empty());
    }
}

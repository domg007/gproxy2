use sea_orm::{
    ColumnTrait, DbErr, EntityTrait, JoinType, Order, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait,
};
use serde_json::Value as JsonValue;

use super::super::entities::{
    credential_statuses, credentials, global_settings, providers, user_keys, users,
};
use super::super::{DatabaseCipher, SeaOrmStorage};
use crate::query::{
    CredentialQuery, CredentialQueryCount, CredentialQueryRow, CredentialStatusQuery,
    CredentialStatusQueryCount, CredentialStatusQueryRow, GlobalSettingsRow, ProviderQuery,
    ProviderQueryRow, Scope, UserKeyMemoryRow, UserKeyQuery, UserKeyQueryRow, UserQuery,
    UserQueryRow,
};

fn decrypt_string_field(
    cipher: Option<&DatabaseCipher>,
    field: &str,
    raw: String,
) -> Result<String, DbErr> {
    match cipher {
        Some(cipher) => cipher
            .decrypt_string(&raw)
            .map_err(|err| DbErr::Custom(format!("decrypt {field}: {err}"))),
        None => Ok(raw),
    }
}

fn decrypt_optional_string_field(
    cipher: Option<&DatabaseCipher>,
    field: &str,
    raw: Option<String>,
) -> Result<Option<String>, DbErr> {
    match (cipher, raw) {
        (_, None) => Ok(None),
        (Some(cipher), Some(raw)) => cipher
            .decrypt_string(&raw)
            .map(Some)
            .map_err(|err| DbErr::Custom(format!("decrypt {field}: {err}"))),
        (None, Some(raw)) => Ok(Some(raw)),
    }
}

fn decrypt_json_field(
    cipher: Option<&DatabaseCipher>,
    field: &str,
    raw: JsonValue,
) -> Result<JsonValue, DbErr> {
    match cipher {
        Some(cipher) => cipher
            .decrypt_json(raw)
            .map_err(|err| DbErr::Custom(format!("decrypt {field}: {err}"))),
        None => Ok(raw),
    }
}

impl SeaOrmStorage {
    pub async fn get_global_settings(&self) -> Result<Option<GlobalSettingsRow>, DbErr> {
        let row = global_settings::Entity::find()
            .order_by(global_settings::Column::UpdatedAt, Order::Desc)
            .one(self.connection())
            .await?;
        let cipher = self.cipher();
        Ok(match row {
            Some(row) => Some(GlobalSettingsRow {
                id: row.id,
                host: row.host,
                port: row.port,
                admin_key: decrypt_string_field(
                    cipher,
                    "global_settings.admin_key",
                    row.admin_key,
                )?,
                hf_token: decrypt_optional_string_field(
                    cipher,
                    "global_settings.hf_token",
                    row.hf_token,
                )?,
                hf_url: row.hf_url,
                proxy: row.proxy,
                spoof_emulation: row.spoof_emulation,
                update_source: row.update_source,
                dsn: row.dsn,
                data_dir: row.data_dir,
                mask_sensitive_info: row.mask_sensitive_info,
                updated_at: row.updated_at,
            }),
            None => None,
        })
    }

    pub async fn list_providers(
        &self,
        query: &ProviderQuery,
    ) -> Result<Vec<ProviderQueryRow>, DbErr> {
        let mut stmt =
            providers::Entity::find().order_by(providers::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(channel) = &query.channel {
            stmt = stmt.filter(providers::Column::Channel.eq(channel.as_str()));
        }
        if let Scope::Eq(name) = &query.name {
            stmt = stmt.filter(providers::Column::Name.eq(name.as_str()));
        }
        if let Scope::Eq(enabled) = query.enabled {
            stmt = stmt.filter(providers::Column::Enabled.eq(enabled));
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| ProviderQueryRow {
                id: row.id,
                name: row.name,
                channel: row.channel,
                settings_json: row.settings_json,
                dispatch_json: row.dispatch_json,
                enabled: row.enabled,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect())
    }

    pub async fn list_credentials(
        &self,
        query: &CredentialQuery,
    ) -> Result<Vec<CredentialQueryRow>, DbErr> {
        let mut stmt =
            credentials::Entity::find().order_by(credentials::Column::UpdatedAt, Order::Desc);
        match &query.id {
            Scope::All => {}
            Scope::Eq(id) => {
                stmt = stmt.filter(credentials::Column::Id.eq(*id));
            }
            Scope::In(ids) => {
                stmt = stmt.filter(credentials::Column::Id.is_in(ids.iter().copied()));
            }
        }
        if let Scope::Eq(provider_id) = query.provider_id {
            stmt = stmt.filter(credentials::Column::ProviderId.eq(provider_id));
        }
        if let Scope::Eq(kind) = &query.kind {
            stmt = stmt.filter(credentials::Column::Kind.eq(kind.as_str()));
        }
        if let Scope::Eq(enabled) = query.enabled {
            stmt = stmt.filter(credentials::Column::Enabled.eq(enabled));
        }
        if let Some(name_contains) = query
            .name_contains
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            stmt = stmt.filter(credentials::Column::Name.contains(name_contains));
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        if let Some(offset) = query.offset
            && offset > 0
        {
            stmt = stmt.offset(offset);
        }
        let rows = stmt.all(self.connection()).await?;
        let cipher = self.cipher();
        rows.into_iter()
            .map(|row| {
                Ok(CredentialQueryRow {
                    id: row.id,
                    provider_id: row.provider_id,
                    name: row.name,
                    kind: row.kind,
                    settings_json: row.settings_json,
                    secret_json: decrypt_json_field(
                        cipher,
                        "credential.secret_json",
                        row.secret_json,
                    )?,
                    enabled: row.enabled,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .collect()
    }

    pub async fn count_credentials(
        &self,
        query: &CredentialQuery,
    ) -> Result<CredentialQueryCount, DbErr> {
        let mut stmt = credentials::Entity::find();
        match &query.id {
            Scope::All => {}
            Scope::Eq(id) => {
                stmt = stmt.filter(credentials::Column::Id.eq(*id));
            }
            Scope::In(ids) => {
                stmt = stmt.filter(credentials::Column::Id.is_in(ids.iter().copied()));
            }
        }
        if let Scope::Eq(provider_id) = query.provider_id {
            stmt = stmt.filter(credentials::Column::ProviderId.eq(provider_id));
        }
        if let Scope::Eq(kind) = &query.kind {
            stmt = stmt.filter(credentials::Column::Kind.eq(kind.as_str()));
        }
        if let Scope::Eq(enabled) = query.enabled {
            stmt = stmt.filter(credentials::Column::Enabled.eq(enabled));
        }
        if let Some(name_contains) = query
            .name_contains
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            stmt = stmt.filter(credentials::Column::Name.contains(name_contains));
        }
        let count = stmt.count(self.connection()).await?;
        Ok(CredentialQueryCount { count })
    }

    pub async fn list_credential_statuses(
        &self,
        query: &CredentialStatusQuery,
    ) -> Result<Vec<CredentialStatusQueryRow>, DbErr> {
        let mut stmt = credential_statuses::Entity::find()
            .order_by(credential_statuses::Column::Id, Order::Desc);
        if credential_status_query_needs_credential_join(query) {
            stmt = stmt.join(
                JoinType::InnerJoin,
                credential_statuses::Relation::Credentials.def(),
            );
        }
        match &query.id {
            Scope::All => {}
            Scope::Eq(id) => {
                stmt = stmt.filter(credential_statuses::Column::Id.eq(*id));
            }
            Scope::In(ids) => {
                stmt = stmt.filter(credential_statuses::Column::Id.is_in(ids.iter().copied()));
            }
        }
        match &query.credential_id {
            Scope::All => {}
            Scope::Eq(credential_id) => {
                stmt = stmt.filter(credential_statuses::Column::CredentialId.eq(*credential_id));
            }
            Scope::In(ids) => {
                stmt = stmt
                    .filter(credential_statuses::Column::CredentialId.is_in(ids.iter().copied()));
            }
        }
        match &query.provider_id {
            Scope::All => {}
            Scope::Eq(provider_id) => {
                stmt = stmt.filter(credentials::Column::ProviderId.eq(*provider_id));
            }
            Scope::In(ids) => {
                stmt = stmt.filter(credentials::Column::ProviderId.is_in(ids.iter().copied()));
            }
        }
        if let Scope::Eq(channel) = &query.channel {
            stmt = stmt.filter(credential_statuses::Column::Channel.eq(channel.as_str()));
        }
        if let Scope::Eq(health_kind) = &query.health_kind {
            stmt = stmt.filter(credential_statuses::Column::HealthKind.eq(health_kind.as_str()));
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        if let Some(offset) = query.offset
            && offset > 0
        {
            stmt = stmt.offset(offset);
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| CredentialStatusQueryRow {
                id: row.id,
                credential_id: row.credential_id,
                channel: row.channel,
                health_kind: row.health_kind,
                health_json: row.health_json,
                checked_at: row.checked_at,
                last_error: row.last_error,
                updated_at: row.updated_at,
            })
            .collect())
    }

    pub async fn count_credential_statuses(
        &self,
        query: &CredentialStatusQuery,
    ) -> Result<CredentialStatusQueryCount, DbErr> {
        let mut stmt = credential_statuses::Entity::find();
        if credential_status_query_needs_credential_join(query) {
            stmt = stmt.join(
                JoinType::InnerJoin,
                credential_statuses::Relation::Credentials.def(),
            );
        }
        match &query.id {
            Scope::All => {}
            Scope::Eq(id) => {
                stmt = stmt.filter(credential_statuses::Column::Id.eq(*id));
            }
            Scope::In(ids) => {
                stmt = stmt.filter(credential_statuses::Column::Id.is_in(ids.iter().copied()));
            }
        }
        match &query.credential_id {
            Scope::All => {}
            Scope::Eq(credential_id) => {
                stmt = stmt.filter(credential_statuses::Column::CredentialId.eq(*credential_id));
            }
            Scope::In(ids) => {
                stmt = stmt
                    .filter(credential_statuses::Column::CredentialId.is_in(ids.iter().copied()));
            }
        }
        match &query.provider_id {
            Scope::All => {}
            Scope::Eq(provider_id) => {
                stmt = stmt.filter(credentials::Column::ProviderId.eq(*provider_id));
            }
            Scope::In(ids) => {
                stmt = stmt.filter(credentials::Column::ProviderId.is_in(ids.iter().copied()));
            }
        }
        if let Scope::Eq(channel) = &query.channel {
            stmt = stmt.filter(credential_statuses::Column::Channel.eq(channel.as_str()));
        }
        if let Scope::Eq(health_kind) = &query.health_kind {
            stmt = stmt.filter(credential_statuses::Column::HealthKind.eq(health_kind.as_str()));
        }
        let count = stmt.count(self.connection()).await?;
        Ok(CredentialStatusQueryCount { count })
    }

    pub async fn list_users(&self, query: &UserQuery) -> Result<Vec<UserQueryRow>, DbErr> {
        let mut stmt = users::Entity::find().order_by(users::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(users::Column::Id.eq(id));
        }
        if let Scope::Eq(name) = &query.name {
            stmt = stmt.filter(users::Column::Name.eq(name.as_str()));
        }
        let rows = stmt.all(self.connection()).await?;
        let cipher = self.cipher();
        rows.into_iter()
            .map(|row| {
                Ok(UserQueryRow {
                    id: row.id,
                    name: row.name,
                    password: decrypt_optional_string_field(cipher, "user.password", row.password)?
                        .unwrap_or_default(),
                    enabled: row.enabled,
                })
            })
            .collect()
    }

    pub async fn list_user_keys(
        &self,
        query: &UserKeyQuery,
    ) -> Result<Vec<UserKeyQueryRow>, DbErr> {
        let mut stmt =
            user_keys::Entity::find().order_by(user_keys::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(user_keys::Column::Id.eq(id));
        }
        if let Scope::Eq(user_id) = query.user_id {
            stmt = stmt.filter(user_keys::Column::UserId.eq(user_id));
        }
        let api_key_filter = if let Scope::Eq(api_key) = &query.api_key {
            Some(api_key.clone())
        } else {
            None
        };
        if self.cipher().is_none()
            && let Some(api_key) = api_key_filter.as_deref()
        {
            stmt = stmt.filter(user_keys::Column::ApiKey.eq(api_key));
        }
        let rows = stmt.all(self.connection()).await?;
        let cipher = self.cipher();
        let mut rows: Vec<UserKeyQueryRow> = rows
            .into_iter()
            .map(|row| {
                Ok(UserKeyQueryRow {
                    id: row.id,
                    user_id: row.user_id,
                    api_key: decrypt_string_field(cipher, "user_key.api_key", row.api_key)?,
                })
            })
            .collect::<Result<_, DbErr>>()?;
        if let Some(api_key) = api_key_filter.as_deref() {
            rows.retain(|row| row.api_key == api_key);
        }
        Ok(rows)
    }

    pub async fn list_user_keys_for_memory(
        &self,
        query: &UserKeyQuery,
    ) -> Result<Vec<UserKeyMemoryRow>, DbErr> {
        let mut stmt =
            user_keys::Entity::find().order_by(user_keys::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(user_keys::Column::Id.eq(id));
        }
        if let Scope::Eq(user_id) = query.user_id {
            stmt = stmt.filter(user_keys::Column::UserId.eq(user_id));
        }
        let api_key_filter = if let Scope::Eq(api_key) = &query.api_key {
            Some(api_key.clone())
        } else {
            None
        };
        if self.cipher().is_none()
            && let Some(api_key) = api_key_filter.as_deref()
        {
            stmt = stmt.filter(user_keys::Column::ApiKey.eq(api_key));
        }
        let rows = stmt.all(self.connection()).await?;
        let cipher = self.cipher();
        let mut rows: Vec<UserKeyMemoryRow> = rows
            .into_iter()
            .map(|row| {
                Ok(UserKeyMemoryRow {
                    id: row.id,
                    user_id: row.user_id,
                    api_key: decrypt_string_field(cipher, "user_key.api_key", row.api_key)?,
                    enabled: row.enabled,
                })
            })
            .collect::<Result<_, DbErr>>()?;
        if let Some(api_key) = api_key_filter.as_deref() {
            rows.retain(|row| row.api_key == api_key);
        }
        Ok(rows)
    }
}

fn credential_status_query_needs_credential_join(query: &CredentialStatusQuery) -> bool {
    !matches!(query.provider_id, Scope::All)
}

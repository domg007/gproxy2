use sea_orm::{
    ActiveModelTrait,
    ActiveValue::{NotSet, Set},
    ColumnTrait, DbErr, EntityTrait, QueryFilter,
};
use serde_json::Value as JsonValue;
use time::OffsetDateTime;

use super::entities::{
    credentials, downstream_requests, providers, upstream_requests, user_keys, users,
};
use super::{DatabaseCipher, SeaOrmStorage};

fn parse_json(field: &str, raw: &str) -> Result<JsonValue, DbErr> {
    serde_json::from_str(raw)
        .map_err(|err| DbErr::Custom(format!("invalid json for {field}: {err}")))
}

fn parse_optional_json(field: &str, raw: Option<&str>) -> Result<Option<JsonValue>, DbErr> {
    raw.map(|value| parse_json(field, value)).transpose()
}

fn encrypt_string_field(
    cipher: Option<&DatabaseCipher>,
    field: &str,
    raw: &str,
) -> Result<String, DbErr> {
    match cipher {
        Some(cipher) => cipher
            .encrypt_string(raw)
            .map_err(|err| DbErr::Custom(format!("encrypt {field}: {err}"))),
        None => Ok(raw.to_string()),
    }
}

fn encrypt_optional_string_field(
    cipher: Option<&DatabaseCipher>,
    field: &str,
    raw: Option<&str>,
) -> Result<Option<String>, DbErr> {
    raw.map(|value| encrypt_string_field(cipher, field, value))
        .transpose()
}

fn encrypt_json_field(
    cipher: Option<&DatabaseCipher>,
    field: &str,
    raw: &str,
) -> Result<JsonValue, DbErr> {
    let value = parse_json(field, raw)?;
    match cipher {
        Some(cipher) => cipher
            .encrypt_json(&value)
            .map_err(|err| DbErr::Custom(format!("encrypt {field}: {err}"))),
        None => Ok(value),
    }
}

impl SeaOrmStorage {
    pub async fn create_provider(
        &self,
        name: &str,
        channel: &str,
        settings_json: &str,
        dispatch_json: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = providers::ActiveModel {
            id: NotSet,
            name: Set(name.to_string()),
            channel: Set(channel.to_string()),
            settings_json: Set(parse_json("provider.settings_json", settings_json)?),
            dispatch_json: Set(parse_json("provider.dispatch_json", dispatch_json)?),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(self.connection())
        .await?;
        Ok(model.id)
    }

    pub async fn create_credential(
        &self,
        provider_id: i64,
        name: Option<&str>,
        kind: &str,
        settings_json: Option<&str>,
        secret_json: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = credentials::ActiveModel {
            id: NotSet,
            provider_id: Set(provider_id),
            name: Set(name.map(str::to_string)),
            kind: Set(kind.to_string()),
            settings_json: Set(parse_optional_json(
                "credential.settings_json",
                settings_json,
            )?),
            secret_json: Set(encrypt_json_field(
                self.cipher(),
                "credential.secret_json",
                secret_json,
            )?),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(self.connection())
        .await?;
        Ok(model.id)
    }

    pub async fn create_user(
        &self,
        name: &str,
        password: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = users::ActiveModel {
            id: NotSet,
            name: Set(name.to_string()),
            password: Set(encrypt_optional_string_field(
                self.cipher(),
                "user.password",
                Some(password),
            )?),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(self.connection())
        .await?;
        Ok(model.id)
    }

    pub async fn create_user_key(
        &self,
        user_id: i64,
        api_key: &str,
        label: Option<&str>,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = user_keys::ActiveModel {
            id: NotSet,
            user_id: Set(user_id),
            api_key: Set(encrypt_string_field(
                self.cipher(),
                "user_key.api_key",
                api_key,
            )?),
            label: Set(label.map(str::to_string)),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(self.connection())
        .await?;
        Ok(model.id)
    }

    pub async fn clear_upstream_request_payloads(
        &self,
        trace_ids: Option<&[i64]>,
    ) -> Result<u64, DbErr> {
        if let Some(ids) = trace_ids
            && ids.is_empty()
        {
            return Ok(0);
        }

        let mut stmt =
            upstream_requests::Entity::update_many().set(upstream_requests::ActiveModel {
                request_headers_json: Set(serde_json::json!({})),
                request_body: Set(None),
                response_headers_json: Set(serde_json::json!({})),
                response_body: Set(None),
                ..Default::default()
            });

        if let Some(ids) = trace_ids {
            stmt = stmt.filter(upstream_requests::Column::TraceId.is_in(ids.iter().copied()));
        }

        Ok(stmt.exec(self.connection()).await?.rows_affected)
    }

    pub async fn clear_downstream_request_payloads(
        &self,
        trace_ids: Option<&[i64]>,
    ) -> Result<u64, DbErr> {
        if let Some(ids) = trace_ids
            && ids.is_empty()
        {
            return Ok(0);
        }

        let mut stmt =
            downstream_requests::Entity::update_many().set(downstream_requests::ActiveModel {
                request_headers_json: Set(serde_json::json!({})),
                request_body: Set(None),
                response_headers_json: Set(serde_json::json!({})),
                response_body: Set(None),
                ..Default::default()
            });

        if let Some(ids) = trace_ids {
            stmt = stmt.filter(downstream_requests::Column::TraceId.is_in(ids.iter().copied()));
        }

        Ok(stmt.exec(self.connection()).await?.rows_affected)
    }
}

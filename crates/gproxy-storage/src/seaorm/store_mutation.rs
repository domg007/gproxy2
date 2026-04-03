use sea_orm::*;
use sea_orm::sea_query::Expr;
use time::OffsetDateTime;

use crate::seaorm::SeaOrmStorage;
use crate::seaorm::entities::*;

impl SeaOrmStorage {
    pub async fn create_provider(
        &self,
        name: &str,
        channel: &str,
        settings_json: &str,
        dispatch_json: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let settings: serde_json::Value = serde_json::from_str(settings_json)
            .map_err(|e| DbErr::Custom(format!("invalid settings_json: {e}")))?;
        let dispatch: serde_json::Value = serde_json::from_str(dispatch_json)
            .map_err(|e| DbErr::Custom(format!("invalid dispatch_json: {e}")))?;
        let now = OffsetDateTime::now_utc();
        let model = providers::ActiveModel {
            name: Set(name.to_string()),
            channel: Set(channel.to_string()),
            settings_json: Set(settings),
            dispatch_json: Set(dispatch),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = providers::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_credential(
        &self,
        provider_id: i64,
        name: Option<&str>,
        kind: &str,
        secret_json: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let secret: serde_json::Value = serde_json::from_str(secret_json)
            .map_err(|e| DbErr::Custom(format!("invalid secret_json: {e}")))?;
        let encrypted_secret = self.encrypt_json(&secret);
        let now = OffsetDateTime::now_utc();
        let model = credentials::ActiveModel {
            provider_id: Set(provider_id),
            name: Set(name.map(String::from)),
            kind: Set(kind.to_string()),
            secret_json: Set(encrypted_secret),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = credentials::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_user(
        &self,
        name: &str,
        password: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let encrypted_password = self.encrypt_string(password);
        let now = OffsetDateTime::now_utc();
        let model = users::ActiveModel {
            name: Set(name.to_string()),
            password: Set(Some(encrypted_password)),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = users::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_user_key(
        &self,
        user_id: i64,
        api_key: &str,
        label: Option<&str>,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let encrypted_key = self.encrypt_string(api_key);
        let now = OffsetDateTime::now_utc();
        let model = user_keys::ActiveModel {
            user_id: Set(user_id),
            api_key: Set(encrypted_key),
            label: Set(label.map(String::from)),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = user_keys::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_model(
        &self,
        provider_id: i64,
        model_id: &str,
        display_name: Option<&str>,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = models::ActiveModel {
            provider_id: Set(provider_id),
            model_id: Set(model_id.to_string()),
            display_name: Set(display_name.map(String::from)),
            enabled: Set(enabled),
            price_input_tokens: Set(None),
            price_output_tokens: Set(None),
            price_cache_read_input_tokens: Set(None),
            price_cache_creation_input_tokens: Set(None),
            price_cache_creation_input_tokens_5min: Set(None),
            price_cache_creation_input_tokens_1h: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = models::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_model_alias(
        &self,
        alias: &str,
        provider_id: i64,
        model_id: &str,
        enabled: bool,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = model_aliases::ActiveModel {
            alias: Set(alias.to_string()),
            provider_id: Set(provider_id),
            model_id: Set(model_id.to_string()),
            enabled: Set(enabled),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = model_aliases::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_user_model_permission(
        &self,
        user_id: i64,
        provider_id: Option<i64>,
        model_pattern: &str,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = user_model_permissions::ActiveModel {
            user_id: Set(user_id),
            provider_id: Set(provider_id),
            model_pattern: Set(model_pattern.to_string()),
            created_at: Set(now),
            ..Default::default()
        };
        let result = user_model_permissions::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn create_user_rate_limit(
        &self,
        user_id: i64,
        model_pattern: &str,
        rpm: Option<i32>,
        rpd: Option<i32>,
        total_tokens: Option<i64>,
    ) -> Result<i64, DbErr> {
        let now = OffsetDateTime::now_utc();
        let model = user_rate_limits::ActiveModel {
            user_id: Set(user_id),
            model_pattern: Set(model_pattern.to_string()),
            rpm: Set(rpm),
            rpd: Set(rpd),
            total_tokens: Set(total_tokens),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let result = user_rate_limits::Entity::insert(model).exec(&self.db).await?;
        Ok(result.last_insert_id)
    }

    pub async fn clear_upstream_request_payloads(
        &self,
        trace_ids: Option<&[i64]>,
    ) -> Result<u64, DbErr> {
        let mut update = upstream_requests::Entity::update_many()
            .col_expr(upstream_requests::Column::RequestBody, Expr::value(Option::<Vec<u8>>::None))
            .col_expr(upstream_requests::Column::ResponseBody, Expr::value(Option::<Vec<u8>>::None));
        if let Some(ids) = trace_ids {
            update = update.filter(upstream_requests::Column::TraceId.is_in(ids.to_vec()));
        }
        let result = update.exec(&self.db).await?;
        Ok(result.rows_affected)
    }

    pub async fn clear_downstream_request_payloads(
        &self,
        trace_ids: Option<&[i64]>,
    ) -> Result<u64, DbErr> {
        let mut update = downstream_requests::Entity::update_many()
            .col_expr(downstream_requests::Column::RequestBody, Expr::value(Option::<Vec<u8>>::None))
            .col_expr(downstream_requests::Column::ResponseBody, Expr::value(Option::<Vec<u8>>::None));
        if let Some(ids) = trace_ids {
            update = update.filter(downstream_requests::Column::TraceId.is_in(ids.to_vec()));
        }
        let result = update.exec(&self.db).await?;
        Ok(result.rows_affected)
    }

    pub async fn delete_upstream_requests(
        &self,
        trace_ids: Option<&[i64]>,
    ) -> Result<u64, DbErr> {
        let mut delete = upstream_requests::Entity::delete_many();
        if let Some(ids) = trace_ids {
            delete = delete.filter(upstream_requests::Column::TraceId.is_in(ids.to_vec()));
        }
        let result = delete.exec(&self.db).await?;
        Ok(result.rows_affected)
    }

    pub async fn delete_downstream_requests(
        &self,
        trace_ids: Option<&[i64]>,
    ) -> Result<u64, DbErr> {
        let mut delete = downstream_requests::Entity::delete_many();
        if let Some(ids) = trace_ids {
            delete = delete.filter(downstream_requests::Column::TraceId.is_in(ids.to_vec()));
        }
        let result = delete.exec(&self.db).await?;
        Ok(result.rows_affected)
    }

    // --- Encryption helpers (write direction) ---

    fn encrypt_string(&self, plaintext: &str) -> String {
        match &self.cipher {
            Some(cipher) => cipher.encrypt_string(plaintext).unwrap_or_else(|_| plaintext.to_string()),
            None => plaintext.to_string(),
        }
    }

    fn encrypt_json(&self, value: &serde_json::Value) -> serde_json::Value {
        match &self.cipher {
            Some(cipher) => cipher.encrypt_json(value).unwrap_or_else(|_| value.clone()),
            None => value.clone(),
        }
    }
}

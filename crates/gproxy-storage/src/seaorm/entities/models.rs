use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

/// Model registry — tracks available models per provider with pricing.
///
/// Currently maintained manually by admin. Frontend will add auto-discovery
/// from upstream model list endpoints in a future release.
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    #[sea_orm(unique_key = "model_provider_model_id")]
    pub provider_id: i64,
    #[sea_orm(unique_key = "model_provider_model_id")]
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub price_each_call: Option<f64>,
    /// Pricing per million tokens (USD or any unit). Null = not priced.
    pub price_input_tokens: Option<f64>,
    pub price_output_tokens: Option<f64>,
    pub price_cache_read_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens_5min: Option<f64>,
    pub price_cache_creation_input_tokens_1h: Option<f64>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    #[sea_orm(belongs_to, from = "provider_id", to = "id", on_delete = "Cascade")]
    pub provider: HasOne<super::providers::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

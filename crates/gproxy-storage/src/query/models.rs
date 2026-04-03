use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelQuery {
    pub id: Scope<i64>,
    pub provider_id: Scope<i64>,
    pub model_id: Scope<String>,
    pub enabled: Scope<bool>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelQueryRow {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub price_each_call: Option<f64>,
    pub price_input_tokens: Option<f64>,
    pub price_output_tokens: Option<f64>,
    pub price_cache_read_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens_5min: Option<f64>,
    pub price_cache_creation_input_tokens_1h: Option<f64>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

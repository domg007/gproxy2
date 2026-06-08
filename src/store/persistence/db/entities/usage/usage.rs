//! `usages` table SeaORM entity (per-request usage row). `cost` is `f64`, so
//! this model cannot derive `Eq`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "usages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub request_id: String,
    pub at: i64,
    pub route_name: Option<String>,
    pub provider_id: Option<i64>,
    pub credential_id: Option<i64>,
    pub org_id: Option<i64>,
    pub team_id: Option<i64>,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub operation: String,
    pub kind: String,
    pub model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_5m_tokens: i64,
    pub cache_creation_1h_tokens: i64,
    pub cost: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

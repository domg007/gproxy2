//! `usage_rollups` table SeaORM entity (accumulated usage bucket). `cost` is
//! `f64`, so this model cannot derive `Eq`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "usage_rollups")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub granularity: String,
    pub bucket_start: i64,
    pub provider_id: Option<i64>,
    pub org_id: Option<i64>,
    pub team_id: Option<i64>,
    pub user_id: Option<i64>,
    pub route_name: Option<String>,
    pub model: Option<String>,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

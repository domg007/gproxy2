//! `quotas` table SeaORM entity. Unique per `(scope, scope_id)`.
//!
//! `Eq` is not derived because `quota_total` / `cost_used` are `f64`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "quotas")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub scope: String,
    pub scope_id: i64,
    pub quota_total: f64,
    pub cost_used: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

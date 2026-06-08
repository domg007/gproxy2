//! `sanitize_rules` table SeaORM entity.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sanitize_rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub provider_id: i64,
    pub pattern: String,
    pub replacement: String,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

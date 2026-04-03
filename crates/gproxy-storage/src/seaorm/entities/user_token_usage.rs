use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

/// User cost quota — allocated budget and cumulative spend.
/// Remaining budget = `quota` - `cost_used`.
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "user_quotas")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    #[sea_orm(unique_key = "user_quota_user")]
    pub user_id: i64,
    /// Total allocated budget (set by admin).
    pub quota: f64,
    /// Cumulative cost consumed (computed from model pricing).
    pub cost_used: f64,
    pub updated_at: OffsetDateTime,
    #[sea_orm(belongs_to, from = "user_id", to = "id", on_delete = "Cascade")]
    pub user: HasOne<super::users::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

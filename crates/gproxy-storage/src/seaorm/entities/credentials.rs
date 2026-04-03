use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "credentials")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    pub provider_id: i64,
    pub name: Option<String>,
    pub kind: String,
    pub secret_json: Json,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    #[sea_orm(belongs_to, from = "provider_id", to = "id", on_delete = "Cascade")]
    pub provider: HasOne<super::providers::Entity>,
    #[sea_orm(has_many)]
    pub upstream_requests: HasMany<super::upstream_requests::Entity>,
    #[sea_orm(has_many)]
    pub usages: HasMany<super::usages::Entity>,
    #[sea_orm(has_many)]
    pub statuses: HasMany<super::credential_statuses::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "credential_statuses")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    #[sea_orm(unique_key = "credential_status_credential_channel")]
    pub credential_id: i64,
    #[sea_orm(unique_key = "credential_status_credential_channel")]
    pub channel: String,
    pub health_kind: String,
    pub health_json: Option<Json>,
    pub checked_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub updated_at: OffsetDateTime,
    #[sea_orm(belongs_to, from = "credential_id", to = "id", on_delete = "Cascade")]
    pub credential: HasOne<super::credentials::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

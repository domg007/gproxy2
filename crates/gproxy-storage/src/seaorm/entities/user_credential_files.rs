use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "user_credential_files")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    #[sea_orm(unique_key = "user_provider_file")]
    pub user_id: i64,
    pub user_key_id: i64,
    #[sea_orm(unique_key = "user_provider_file")]
    pub provider_id: i64,
    pub credential_id: i64,
    #[sea_orm(unique_key = "user_provider_file")]
    pub file_id: String,
    pub active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub deleted_at: Option<OffsetDateTime>,
    #[sea_orm(belongs_to, from = "user_id", to = "id", on_delete = "Cascade")]
    pub user: HasOne<super::users::Entity>,
    #[sea_orm(belongs_to, from = "user_key_id", to = "id", on_delete = "Cascade")]
    pub user_key: HasOne<super::user_keys::Entity>,
    #[sea_orm(belongs_to, from = "provider_id", to = "id", on_delete = "Cascade")]
    pub provider: HasOne<super::providers::Entity>,
    #[sea_orm(belongs_to, from = "credential_id", to = "id", on_delete = "Cascade")]
    pub credential: HasOne<super::credentials::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "claude_files")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    #[sea_orm(unique_key = "claude_provider_file")]
    pub provider_id: i64,
    #[sea_orm(unique_key = "claude_provider_file")]
    pub file_id: String,
    pub file_created_at: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub downloadable: Option<bool>,
    pub raw_json: Json,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    #[sea_orm(belongs_to, from = "provider_id", to = "id", on_delete = "Cascade")]
    pub provider: HasOne<super::providers::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

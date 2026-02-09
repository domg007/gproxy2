use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "global_config")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub host: String,
    pub port: i32,
    #[sea_orm(column_name = "admin_key_hash")]
    pub admin_key: String,
    pub proxy: Option<String>,
    pub dsn: String,
    pub event_redact_sensitive: Option<bool>,
    pub updated_at: OffsetDateTime,
}

impl ActiveModelBehavior for ActiveModel {}

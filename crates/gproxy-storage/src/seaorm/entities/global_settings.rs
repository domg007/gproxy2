use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "global_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    pub host: String,
    pub port: i32,
    pub admin_key: String,
    pub hf_token: Option<String>,
    pub hf_url: Option<String>,
    pub proxy: Option<String>,
    pub dsn: String,
    pub data_dir: String,
    pub mask_sensitive_info: bool,
    pub updated_at: OffsetDateTime,
}

impl ActiveModelBehavior for ActiveModel {}

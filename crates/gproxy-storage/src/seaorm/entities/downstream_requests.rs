use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "downstream_requests")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub trace_id: i64,
    pub at: OffsetDateTime,
    pub internal: bool,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub request_method: String,
    pub request_headers_json: Json,
    pub request_path: String,
    pub request_query: Option<String>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<i32>,
    pub response_headers_json: Json,
    pub response_body: Option<Vec<u8>>,
    pub created_at: OffsetDateTime,
    #[sea_orm(belongs_to, from = "user_id", to = "id", on_delete = "SetNull")]
    pub user: HasOne<super::users::Entity>,
    #[sea_orm(belongs_to, from = "user_key_id", to = "id", on_delete = "SetNull")]
    pub user_key: HasOne<super::user_keys::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}

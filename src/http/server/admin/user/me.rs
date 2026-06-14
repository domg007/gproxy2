//! `GET /user/me` — returns the session user's identity.

use axum::{Extension, Json};

use crate::admin::session::SessionUser;

/// Portal identity shape returned by `GET /user/me`.
#[derive(serde::Serialize)]
pub struct UserMe {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
    pub org_id: i64,
    pub team_id: Option<i64>,
}

/// `GET /user/me` — reflect the session identity back to the caller.
/// `user_id` always comes from the validated `SessionUser` extension;
/// no request-supplied id is accepted.
pub async fn me(Extension(u): Extension<SessionUser>) -> Json<UserMe> {
    Json(UserMe {
        id: u.id,
        name: u.name,
        is_admin: u.is_admin,
        org_id: u.org_id,
        team_id: u.team_id,
    })
}

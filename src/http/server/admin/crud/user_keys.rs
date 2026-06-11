//! User-keys CRUD — special-cased: writes derive a digest from the BARE key
//! and SEAL the ciphertext ([`UserKeyUpsert`]); reads expose only a short
//! `key_prefix` ([`UserKeyView`]). On an update with no `api_key`, the stored
//! digest + ciphertext are preserved.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::internal;
use crate::admin::invalidate;
use crate::api::error::ApiError;
use crate::api::user_keys::{UserKeyUpsert, UserKeyView};
use crate::app::AppState;
use crate::pipeline::auth::key_digest;
use crate::store::persistence::records::UserKeyInput;

/// `GET /admin/users/{user_id}/keys` — redacted list.
pub async fn list(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<Vec<UserKeyView>>, ApiError> {
    let keys = state
        .persistence
        .list_user_keys(user_id)
        .await
        .map_err(internal)?;
    Ok(Json(keys.into_iter().map(UserKeyView::from).collect()))
}

/// `POST /admin/users/{user_id}/keys` — create or update. The bare `api_key`
/// is digested + sealed; when omitted on an update the stored digest +
/// ciphertext are kept; on create it is required (400 otherwise). The redacted
/// view is returned.
pub async fn upsert(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(body): Json<UserKeyUpsert>,
) -> Result<Json<UserKeyView>, ApiError> {
    // Resolve the (digest, ciphertext) pair to store.
    let (digest, ciphertext) = match (&body.api_key, body.id) {
        // New bare key supplied → digest + seal it.
        (Some(bare), _) => {
            let digest = key_digest(bare);
            let sealed = state
                .cipher
                .seal(&serde_json::Value::String(bare.clone()))
                .map_err(internal)?;
            let ciphertext = match &sealed {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).map_err(internal)?,
            };
            (digest, ciphertext)
        }
        // No key on update → keep the existing digest + ciphertext.
        (None, Some(id)) => {
            let existing = state
                .persistence
                .get_user_key(id)
                .await
                .map_err(internal)?
                .filter(|k| k.user_id == user_id)
                .ok_or_else(|| ApiError::NotFound("not found".into()))?;
            (existing.api_key_digest, existing.api_key_ciphertext)
        }
        // No key on create → reject.
        (None, None) => {
            return Err(ApiError::BadRequest("api_key required on create".into()));
        }
    };

    let input = UserKeyInput {
        id: body.id,
        user_id,
        api_key_digest: digest,
        api_key_ciphertext: ciphertext,
        label: body.label,
        enabled: body.enabled,
    };
    let key = state
        .persistence
        .upsert_user_key(input)
        .await
        .map_err(internal)?;
    invalidate(&state).await;
    Ok(Json(UserKeyView::from(key)))
}

/// `DELETE /admin/user-keys/{id}` — 204 on removal, 404 otherwise.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::response::Response, ApiError> {
    if state
        .persistence
        .delete_user_key(id)
        .await
        .map_err(internal)?
    {
        invalidate(&state).await;
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(ApiError::NotFound("not found".into()))
    }
}

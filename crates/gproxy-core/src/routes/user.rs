use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde::Serialize;

use crate::AppState;

use super::error::HttpError;

const X_API_KEY: &str = "x-api-key";

#[derive(Debug, Serialize)]
struct Ack {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteMyKeyPayload {
    id: i64,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/keys/query", post(query_my_keys))
        .route("/keys/upsert", post(upsert_my_key))
        .route("/keys/delete", post(delete_my_key))
        .route("/usages/query", post(query_my_usages))
        .route("/usages/summary", post(summarize_my_usages))
}

fn api_key_from_headers(headers: &HeaderMap) -> Result<&str, HttpError> {
    gproxy_admin::extract_api_key(headers.get(X_API_KEY).and_then(|value| value.to_str().ok()))
        .map_err(Into::into)
}

async fn query_my_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<gproxy_storage::UserKeyQueryRow>>, HttpError> {
    let api_key = api_key_from_headers(&headers)?;
    let users = state.load_users();
    let keys = state.load_keys();
    Ok(Json(
        gproxy_admin::query_my_user_keys(&users, &keys, api_key).await?,
    ))
}

async fn upsert_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_admin::UpsertMyKeyInput>,
) -> Result<Json<gproxy_storage::UserKeyWrite>, HttpError> {
    let api_key = api_key_from_headers(&headers)?;
    let users = state.load_users();
    let keys = state.load_keys();
    let row =
        gproxy_admin::upsert_my_user_key(&state.storage_writes, api_key, &users, &keys, payload)
            .await?;
    state.upsert_user_key_in_memory(row.clone());
    Ok(Json(row))
}

async fn delete_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteMyKeyPayload>,
) -> Result<Json<Ack>, HttpError> {
    let api_key = api_key_from_headers(&headers)?;
    let users = state.load_users();
    let keys = state.load_keys();
    gproxy_admin::delete_my_user_key(&state.storage_writes, api_key, &users, &keys, payload.id)
        .await?;
    state.delete_user_key_in_memory(payload.id);
    Ok(Json(Ack { ok: true }))
}

async fn query_my_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UsageQuery>,
) -> Result<Json<Vec<gproxy_storage::UsageQueryRow>>, HttpError> {
    let api_key = api_key_from_headers(&headers)?;
    let users = state.load_users();
    let keys = state.load_keys();
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_my_usages(&storage, &users, &keys, api_key, query).await?,
    ))
}

async fn summarize_my_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UsageQuery>,
) -> Result<Json<gproxy_storage::UsageSummary>, HttpError> {
    let api_key = api_key_from_headers(&headers)?;
    let users = state.load_users();
    let keys = state.load_keys();
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::summarize_my_usages(&storage, &users, &keys, api_key, query).await?,
    ))
}

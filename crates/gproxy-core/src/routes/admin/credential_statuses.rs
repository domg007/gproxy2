use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use gproxy_provider::{ChannelCredentialState, ChannelId, CredentialHealth, ModelCooldown};

use crate::AppState;

use super::{Ack, DeleteCredentialStatusPayload, HttpError, authorize_admin};

pub(super) async fn query_credential_statuses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::CredentialStatusQuery>,
) -> Result<Json<Vec<gproxy_storage::CredentialStatusQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_credential_statuses(&storage, query).await?,
    ))
}

pub(super) async fn upsert_credential_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::CredentialStatusWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let health =
        parse_credential_health(payload.health_kind.as_str(), payload.health_json.as_deref())?;
    let checked_at_unix_ms = payload
        .checked_at_unix_ms
        .and_then(|value| (value >= 0).then_some(value as u64));
    state.upsert_credential_state(ChannelCredentialState {
        channel: ChannelId::parse(payload.channel.as_str()),
        credential_id: payload.credential_id,
        health,
        checked_at_unix_ms,
        last_error: payload.last_error.clone(),
    });
    gproxy_admin::upsert_credential_status(state.storage_writes(), payload).await?;
    Ok(Json(Ack { ok: true }))
}

pub(super) async fn delete_credential_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteCredentialStatusPayload>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    if let Some(row) = gproxy_admin::query_credential_statuses(
        &storage,
        gproxy_storage::CredentialStatusQuery {
            id: gproxy_storage::Scope::Eq(payload.id),
            credential_id: gproxy_storage::Scope::All,
            channel: gproxy_storage::Scope::All,
            health_kind: gproxy_storage::Scope::All,
            limit: Some(1),
        },
    )
    .await?
    .into_iter()
    .next()
    {
        state
            .credential_states()
            .remove(&ChannelId::parse(row.channel.as_str()), row.credential_id);
    }
    gproxy_admin::delete_credential_status(state.storage_writes(), payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

pub(super) fn parse_credential_health(
    kind: &str,
    health_json: Option<&str>,
) -> Result<CredentialHealth, HttpError> {
    let normalized = kind.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "healthy" => Ok(CredentialHealth::Healthy),
        "dead" => Ok(CredentialHealth::Dead),
        "partial" => {
            let raw = health_json.unwrap_or("[]").trim();
            if raw.is_empty() {
                return Ok(CredentialHealth::Partial { models: Vec::new() });
            }
            if let Ok(models) = serde_json::from_str::<Vec<ModelCooldown>>(raw) {
                return Ok(CredentialHealth::Partial { models });
            }
            let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid health_json: {err}"),
                )
            })?;
            let models = value.get("models").cloned().unwrap_or(value);
            let parsed = serde_json::from_value::<Vec<ModelCooldown>>(models).map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid partial models: {err}"),
                )
            })?;
            Ok(CredentialHealth::Partial { models: parsed })
        }
        _ => Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            format!("invalid health_kind: {kind}"),
        )),
    }
}

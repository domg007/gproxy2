use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use gproxy_provider::ChannelId;

use crate::AppState;

use super::{Ack, DeleteById, HttpError, authorize_admin, resolve_provider_channel_by_id};

pub(super) async fn query_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::CredentialQuery>,
) -> Result<Json<Vec<gproxy_storage::CredentialQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_credentials(&storage, query).await?,
    ))
}

pub(super) async fn upsert_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<gproxy_storage::CredentialWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    if let Some(channel) = resolve_provider_channel_by_id(&state, payload.provider_id).await? {
        let mut credential = serde_json::from_str::<gproxy_provider::ChannelCredential>(
            payload.secret_json.as_str(),
        )
        .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;

        maybe_detect_and_fill_project_id(&state, &channel, &mut credential).await?;
        payload.secret_json = serde_json::to_string(&credential)
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;

        state.upsert_provider_credential_in_memory(
            &channel,
            gproxy_provider::CredentialRef {
                id: payload.id,
                label: payload.name.clone(),
                credential,
            },
        );
    }
    gproxy_admin::upsert_credential(&state.storage_writes, payload).await?;
    Ok(Json(Ack { ok: true }))
}

pub(super) async fn maybe_detect_and_fill_project_id(
    state: &AppState,
    channel: &ChannelId,
    credential: &mut gproxy_provider::ChannelCredential,
) -> Result<(), HttpError> {
    let settings = if let Some(provider) = state.config.load().providers.get(channel) {
        provider.settings.clone()
    } else {
        gproxy_provider::parse_provider_settings_json_for_channel(channel, "{}")
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?
    };

    match (channel, credential) {
        (
            ChannelId::Builtin(gproxy_provider::BuiltinChannel::GeminiCli),
            gproxy_provider::ChannelCredential::Builtin(
                gproxy_provider::BuiltinChannelCredential::GeminiCli(value),
            ),
        ) if value.project_id.trim().is_empty() => {
            let http = state.load_http();
            gproxy_provider::channels::geminicli::ensure_geminicli_project_id(
                http.as_ref(),
                &settings,
                value,
            )
            .await
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        (
            ChannelId::Builtin(gproxy_provider::BuiltinChannel::Antigravity),
            gproxy_provider::ChannelCredential::Builtin(
                gproxy_provider::BuiltinChannelCredential::Antigravity(value),
            ),
        ) if value.project_id.trim().is_empty() => {
            let http = state.load_http();
            gproxy_provider::channels::antigravity::ensure_antigravity_project_id(
                http.as_ref(),
                &settings,
                value,
            )
            .await
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        _ => {}
    }

    Ok(())
}

pub(super) async fn delete_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    let rows = storage
        .list_credentials(&gproxy_storage::CredentialQuery {
            provider_id: gproxy_storage::Scope::All,
            kind: gproxy_storage::Scope::All,
            enabled: gproxy_storage::Scope::All,
            limit: None,
        })
        .await
        .map_err(|err| HttpError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if let Some(credential_row) = rows.into_iter().find(|row| row.id == payload.id)
        && let Some(channel) =
            resolve_provider_channel_by_id(&state, credential_row.provider_id).await?
    {
        let _ = state.delete_provider_credential_in_memory(&channel, payload.id);
    }
    gproxy_admin::delete_credential(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

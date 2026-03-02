use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use gproxy_provider::{
    BuiltinChannelCredential, ChannelCredential, ChannelCredentialState, ChannelId,
    CredentialHealth, CredentialRef, ProviderDispatchTable, credential_health_from_storage,
    credential_health_to_storage, credential_kind_for_storage,
    parse_credential_pick_mode_from_provider_settings_value,
};
use gproxy_storage::Scope;

use crate::{
    AppState, build_claudecode_spoof_client, build_http_client, normalize_spoof_emulation,
};

use super::{
    Ack, ExportBootstrapConfig, ExportChannelConfig, ExportCredentialConfig,
    ExportCredentialHealth, ExportCredentialState, ExportGlobalConfig, ExportRuntimeConfig,
    HttpError, ImportBootstrapConfig, ImportChannelConfig, ImportCredentialConfig,
    ImportCredentialHealth, ImportGlobalConfig, ImportTomlPayload, authorize_admin,
};

const CUSTOM_PROVIDER_ID_START: i64 = 1000;

pub(super) async fn export_config_toml(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_admin(&headers, &state)?;

    let snapshot = state.config.load();
    let storage = state.load_storage();
    let mut providers = gproxy_admin::query_providers(
        &storage,
        gproxy_storage::ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    providers.sort_by_key(|row| row.id);

    let mut credentials = gproxy_admin::query_credentials(
        &storage,
        gproxy_storage::CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    credentials.sort_by_key(|row| row.id);

    let statuses = gproxy_admin::query_credential_statuses(
        &storage,
        gproxy_storage::CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
        },
    )
    .await?;
    let status_by_credential_channel = statuses
        .into_iter()
        .map(|row| ((row.credential_id, row.channel.clone()), row))
        .collect::<std::collections::HashMap<_, _>>();

    let channels = providers
        .into_iter()
        .map(|provider| {
            let channel_id = ChannelId::parse(provider.channel.as_str());
            let dispatch = serde_json::from_value::<ProviderDispatchTable>(provider.dispatch_json)
                .map_err(|err| {
                    HttpError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!(
                            "invalid dispatch_json for provider channel={} id={}: {err}",
                            provider.channel, provider.id
                        ),
                    )
                })?;
            let default_dispatch = match channel_id {
                ChannelId::Builtin(builtin) => ProviderDispatchTable::default_for_builtin(builtin),
                ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
            };
            let dispatch = (dispatch != default_dispatch).then_some(dispatch);

            let provider_credentials = credentials
                .iter()
                .filter(|item| item.provider_id == provider.id)
                .map(|row| {
                    let credential = serde_json::from_value::<ChannelCredential>(
                        row.secret_json.clone(),
                    )
                    .map_err(|err| {
                        HttpError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!(
                                "invalid credential secret_json for credential_id={}: {err}",
                                row.id
                            ),
                        )
                    })?;

                    let (secret, builtin) = split_export_credential(credential);
                    let state = status_by_credential_channel
                        .get(&(row.id, provider.channel.clone()))
                        .map(export_credential_state);

                    Ok::<ExportCredentialConfig, HttpError>(ExportCredentialConfig {
                        id: Some(row.id.to_string()),
                        label: row.name.clone(),
                        secret,
                        builtin,
                        state,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<ExportChannelConfig, HttpError>(ExportChannelConfig {
                id: provider.channel,
                enabled: provider.enabled,
                settings: provider.settings_json,
                dispatch,
                credentials: provider_credentials,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    const DEFAULT_STORAGE_WRITE_QUEUE_CAPACITY: usize = 4096;
    const DEFAULT_STORAGE_WRITE_MAX_BATCH_SIZE: usize = 1024;
    const DEFAULT_STORAGE_WRITE_AGGREGATE_WINDOW_MS: u64 = 25;

    let config = ExportBootstrapConfig {
        global: ExportGlobalConfig {
            host: snapshot.global.host.clone(),
            port: snapshot.global.port,
            proxy: snapshot.global.proxy.clone().unwrap_or_default(),
            spoof_emulation: snapshot.global.spoof_emulation.clone(),
            hf_token: snapshot.global.hf_token.clone().unwrap_or_default(),
            hf_url: snapshot.global.hf_url.clone().unwrap_or_default(),
            admin_key: snapshot.global.admin_key.clone(),
            mask_sensitive_info: snapshot.global.mask_sensitive_info,
            dsn: snapshot.global.dsn.clone(),
            data_dir: snapshot.global.data_dir.clone(),
        },
        runtime: ExportRuntimeConfig {
            storage_write_queue_capacity: DEFAULT_STORAGE_WRITE_QUEUE_CAPACITY,
            storage_write_max_batch_size: DEFAULT_STORAGE_WRITE_MAX_BATCH_SIZE,
            storage_write_aggregate_window_ms: DEFAULT_STORAGE_WRITE_AGGREGATE_WINDOW_MS,
        },
        channels,
    };

    let text = toml::to_string_pretty(&config).map_err(|err| {
        HttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize toml failed: {err}"),
        )
    })?;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; charset=utf-8")
        .header(
            "content-disposition",
            "attachment; filename=\"gproxy.toml\"",
        )
        .body(Body::from(text))
        .map_err(|err| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build response failed: {err}"),
            )
        })
}

pub(super) async fn import_config_toml(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ImportTomlPayload>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;

    let parsed: ImportBootstrapConfig = toml::from_str(payload.toml.as_str()).map_err(|err| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            format!("invalid toml payload: {err}"),
        )
    })?;

    apply_imported_global(&state, &parsed.global).await?;
    apply_imported_channels(&state, &parsed.channels).await?;
    Ok(Json(Ack { ok: true }))
}

pub(super) async fn apply_imported_global(
    state: &Arc<AppState>,
    imported: &ImportGlobalConfig,
) -> Result<(), HttpError> {
    let mut global = state.config.load().global.clone();

    if let Some(host) = imported.host.as_ref() {
        global.host = host.clone();
    }
    if let Some(port) = imported.port {
        global.port = port;
    }
    if let Some(proxy) = imported.proxy.as_ref() {
        global.proxy = if proxy.trim().is_empty() {
            None
        } else {
            Some(proxy.clone())
        };
    }
    if let Some(spoof_emulation) = imported.spoof_emulation.as_ref() {
        global.spoof_emulation = normalize_spoof_emulation(Some(spoof_emulation.as_str()));
    }
    if let Some(hf_token) = imported.hf_token.as_ref() {
        global.hf_token = if hf_token.trim().is_empty() {
            None
        } else {
            Some(hf_token.clone())
        };
    }
    if let Some(hf_url) = imported.hf_url.as_ref() {
        global.hf_url = if hf_url.trim().is_empty() {
            None
        } else {
            Some(hf_url.clone())
        };
    }
    if let Some(admin_key) = imported.admin_key.as_ref() {
        global.admin_key = admin_key.clone();
    }
    if let Some(mask_sensitive_info) = imported.mask_sensitive_info {
        global.mask_sensitive_info = mask_sensitive_info;
    }
    if let Some(dsn) = imported.dsn.as_ref() {
        global.dsn = dsn.clone();
    }
    if let Some(data_dir) = imported.data_dir.as_ref() {
        global.data_dir = data_dir.clone();
    }

    let http = Arc::new(build_http_client(global.proxy.as_deref()).map_err(|err| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            format!("build standard upstream http client failed: {err}"),
        )
    })?);
    let spoof_http = Arc::new(
        build_claudecode_spoof_client(global.proxy.as_deref(), global.spoof_emulation.as_str())
            .map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    format!("build claudecode spoof http client failed: {err}"),
                )
            })?,
    );

    gproxy_admin::upsert_global_settings(
        &state.storage_writes,
        gproxy_storage::GlobalSettingsWrite {
            host: global.host.clone(),
            port: global.port,
            proxy: global.proxy.clone(),
            spoof_emulation: global.spoof_emulation.clone(),
            hf_token: global.hf_token.clone(),
            hf_url: global.hf_url.clone(),
            admin_key: global.admin_key.clone(),
            mask_sensitive_info: global.mask_sensitive_info,
            dsn: global.dsn.clone(),
            data_dir: global.data_dir.clone(),
        },
    )
    .await?;

    let mut snapshot = (*state.config.load_full()).clone();
    snapshot.global = global;
    state.replace_config(snapshot);
    state.replace_http_clients(http, spoof_http);

    Ok(())
}

pub(super) async fn apply_imported_channels(
    state: &Arc<AppState>,
    channels: &[ImportChannelConfig],
) -> Result<(), HttpError> {
    if channels.is_empty() {
        return Ok(());
    }

    let storage = state.load_storage();
    let existing_providers = gproxy_admin::query_providers(
        &storage,
        gproxy_storage::ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    let mut provider_id_by_channel = existing_providers
        .iter()
        .map(|row| (row.channel.clone(), row.id))
        .collect::<HashMap<_, _>>();
    let mut used_provider_ids = existing_providers
        .iter()
        .map(|row| row.id)
        .collect::<HashSet<_>>();
    let mut next_provider_id = existing_providers
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;
    let mut next_custom_provider_id = next_provider_id.max(CUSTOM_PROVIDER_ID_START);

    let existing_credentials = gproxy_admin::query_credentials(
        &storage,
        gproxy_storage::CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    let mut credential_id_by_provider_label = existing_credentials
        .iter()
        .filter_map(|row| {
            row.name
                .as_ref()
                .map(|label| ((row.provider_id, label.clone()), row.id))
        })
        .collect::<HashMap<_, _>>();
    let mut used_credential_ids = existing_credentials
        .iter()
        .map(|row| row.id)
        .collect::<HashSet<_>>();
    let mut next_credential_id = existing_credentials
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let existing_statuses = gproxy_admin::query_credential_statuses(
        &storage,
        gproxy_storage::CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
        },
    )
    .await?;
    let status_id_by_credential_channel = existing_statuses
        .into_iter()
        .map(|row| ((row.credential_id, row.channel.clone()), row.id))
        .collect::<HashMap<_, _>>();

    for item in channels {
        let channel_name = item.id.trim();
        if channel_name.is_empty() {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "channel id cannot be empty".to_string(),
            ));
        }
        let channel = ChannelId::parse(channel_name);
        let provider_id = if let Some(existing) = provider_id_by_channel.get(channel_name).copied()
        {
            existing
        } else {
            let id = match channel {
                ChannelId::Builtin(_) => {
                    while used_provider_ids.contains(&next_provider_id) {
                        next_provider_id += 1;
                    }
                    let id = next_provider_id;
                    used_provider_ids.insert(id);
                    next_provider_id += 1;
                    id
                }
                ChannelId::Custom(_) => {
                    while used_provider_ids.contains(&next_custom_provider_id) {
                        next_custom_provider_id += 1;
                    }
                    let id = next_custom_provider_id;
                    used_provider_ids.insert(id);
                    next_custom_provider_id += 1;
                    id
                }
            };
            provider_id_by_channel.insert(channel_name.to_string(), id);
            id
        };

        let settings =
            gproxy_provider::parse_provider_settings_value_for_channel(&channel, &item.settings)
                .map_err(|err| {
                    HttpError::new(
                        StatusCode::BAD_REQUEST,
                        format!("invalid channel settings for {channel_name}: {err}"),
                    )
                })?;
        let credential_pick_mode =
            parse_credential_pick_mode_from_provider_settings_value(&item.settings);
        let dispatch = item.dispatch.clone().unwrap_or_else(|| match channel {
            ChannelId::Builtin(builtin) => ProviderDispatchTable::default_for_builtin(builtin),
            ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
        });

        state.upsert_provider_in_memory(
            channel.clone(),
            settings.clone(),
            dispatch.clone(),
            credential_pick_mode,
            item.enabled,
        );
        let settings_json =
            gproxy_provider::provider_settings_to_json_string_with_credential_pick_mode(
                &settings,
                credential_pick_mode,
            )
            .map_err(|err| {
                HttpError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("serialize provider settings failed for {channel_name}: {err}"),
                )
            })?;
        let dispatch_json = serde_json::to_string(&dispatch).map_err(|err| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize dispatch failed for {channel_name}: {err}"),
            )
        })?;
        gproxy_admin::upsert_provider(
            &state.storage_writes,
            gproxy_storage::ProviderWrite {
                id: provider_id,
                name: channel_name.to_string(),
                channel: channel_name.to_string(),
                settings_json,
                dispatch_json,
                enabled: item.enabled,
            },
        )
        .await?;

        for credential_item in &item.credentials {
            let credential = build_import_channel_credential(&channel, credential_item)?;
            let credential_id = resolve_import_credential_id(
                provider_id,
                credential_item,
                &credential_id_by_provider_label,
                &mut used_credential_ids,
                &mut next_credential_id,
            );

            if let Some(label) = credential_item.label.clone() {
                credential_id_by_provider_label.insert((provider_id, label), credential_id);
            }

            state.upsert_provider_credential_in_memory(
                &channel,
                CredentialRef {
                    id: credential_id,
                    label: credential_item.label.clone(),
                    credential: credential.clone(),
                },
            );
            gproxy_admin::upsert_credential(
                &state.storage_writes,
                gproxy_storage::CredentialWrite {
                    id: credential_id,
                    provider_id,
                    name: credential_item.label.clone(),
                    kind: credential_kind_for_storage(&credential),
                    settings_json: None,
                    secret_json: serde_json::to_string(&credential).map_err(|err| {
                        HttpError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("serialize credential failed: {err}"),
                        )
                    })?,
                    enabled: true,
                },
            )
            .await?;

            if let Some(status) = credential_item.state.as_ref() {
                let (runtime_health, health_kind, health_json) =
                    import_health_to_storage(&status.health);
                state.upsert_credential_state(ChannelCredentialState {
                    channel: channel.clone(),
                    credential_id,
                    health: runtime_health,
                    checked_at_unix_ms: status.checked_at_unix_ms,
                    last_error: status.last_error.clone(),
                });
                gproxy_admin::upsert_credential_status(
                    &state.storage_writes,
                    gproxy_storage::CredentialStatusWrite {
                        id: status_id_by_credential_channel
                            .get(&(credential_id, channel_name.to_string()))
                            .copied(),
                        credential_id,
                        channel: channel_name.to_string(),
                        health_kind,
                        health_json,
                        checked_at_unix_ms: status
                            .checked_at_unix_ms
                            .map(|value| value.min(i64::MAX as u64) as i64),
                        last_error: status.last_error.clone(),
                    },
                )
                .await?;
            }
        }
    }

    Ok(())
}

pub(super) fn resolve_import_credential_id(
    provider_id: i64,
    credential: &ImportCredentialConfig,
    credential_id_by_provider_label: &HashMap<(i64, String), i64>,
    used_credential_ids: &mut HashSet<i64>,
    next_credential_id: &mut i64,
) -> i64 {
    if let Some(id) = credential
        .id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
    {
        used_credential_ids.insert(id);
        if id >= *next_credential_id {
            *next_credential_id = id + 1;
        }
        return id;
    }

    if let Some(label) = credential
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        && let Some(id) = credential_id_by_provider_label
            .get(&(provider_id, label))
            .copied()
    {
        used_credential_ids.insert(id);
        return id;
    }

    while used_credential_ids.contains(next_credential_id) {
        *next_credential_id += 1;
    }
    let id = *next_credential_id;
    used_credential_ids.insert(id);
    *next_credential_id += 1;
    id
}

pub(super) fn build_import_channel_credential(
    channel: &ChannelId,
    credential: &ImportCredentialConfig,
) -> Result<ChannelCredential, HttpError> {
    if let Some(builtin) = credential.builtin.clone() {
        return match channel {
            ChannelId::Builtin(_) => Ok(ChannelCredential::Builtin(builtin)),
            ChannelId::Custom(_) => Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "custom channel does not support builtin credential payload".to_string(),
            )),
        };
    }

    let Some(secret) = credential
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "credential requires either builtin or secret".to_string(),
        ));
    };

    gproxy_provider::credential_from_secret(channel, secret).ok_or_else(|| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "channel {} requires builtin credential object",
                channel.as_str()
            ),
        )
    })
}

pub(super) fn import_health_to_storage(
    health: &ImportCredentialHealth,
) -> (CredentialHealth, String, Option<String>) {
    let runtime_health = match health {
        ImportCredentialHealth::Healthy => CredentialHealth::Healthy,
        ImportCredentialHealth::Dead => CredentialHealth::Dead,
        ImportCredentialHealth::Partial { models } => CredentialHealth::Partial {
            models: models.clone(),
        },
    };
    let (health_kind, health_json) = credential_health_to_storage(&runtime_health);
    (runtime_health, health_kind, health_json)
}

pub(super) fn split_export_credential(
    credential: ChannelCredential,
) -> (Option<String>, Option<BuiltinChannelCredential>) {
    match credential {
        ChannelCredential::Custom(value) => (Some(value.api_key), None),
        ChannelCredential::Builtin(value) => match value {
            BuiltinChannelCredential::OpenAi(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Claude(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::AiStudio(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::VertexExpress(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Nvidia(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Deepseek(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Groq(item) => (Some(item.api_key), None),
            other => (None, Some(other)),
        },
    }
}

pub(super) fn export_credential_state(
    row: &gproxy_storage::CredentialStatusQueryRow,
) -> ExportCredentialState {
    let health = match parse_credential_health_from_status_row(row) {
        CredentialHealth::Healthy => ExportCredentialHealth::Healthy,
        CredentialHealth::Partial { models } => ExportCredentialHealth::Partial { models },
        CredentialHealth::Dead => ExportCredentialHealth::Dead,
    };
    let checked_at_unix_ms = row.checked_at.and_then(|value| {
        let unix_ms = value.unix_timestamp_nanos() / 1_000_000;
        if unix_ms < 0 {
            return None;
        }
        u64::try_from(unix_ms).ok()
    });
    ExportCredentialState {
        health,
        checked_at_unix_ms,
        last_error: row.last_error.clone(),
    }
}

pub(super) fn parse_credential_health_from_status_row(
    row: &gproxy_storage::CredentialStatusQueryRow,
) -> CredentialHealth {
    credential_health_from_storage(row.health_kind.as_str(), row.health_json.as_ref())
        .unwrap_or(CredentialHealth::Healthy)
}

use super::*;

pub(in crate::routes::provider) async fn oauth_start(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let request = UpstreamOAuthRequest {
        query,
        headers: collect_headers(&headers),
    };
    let (response_result, tracked_http_events) = capture_tracked_http_events(async {
        provider.execute_oauth_start(http.as_ref(), &request).await
    })
    .await;
    let response = match response_result {
        Ok(response) => response,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        None,
        response.request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(response.status_code),
            headers: response.headers.as_slice(),
            body: Some(response.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        None,
        tracked_http_events.as_slice(),
        response.request_meta.as_ref(),
    )
    .await;
    Ok(oauth_response_to_axum(response))
}

pub(in crate::routes::provider) async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let request = UpstreamOAuthRequest {
        query,
        headers: collect_headers(&headers),
    };
    let (callback_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_oauth_callback(http.as_ref(), &request)
            .await
    })
    .await;
    let result = match callback_result {
        Ok(result) => result,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    let mut resolved_credential_id: Option<i64> = None;

    if let Some(oauth_credential) = result.credential.as_ref() {
        let provisional = CredentialRef {
            id: -1,
            label: oauth_credential.label.clone(),
            credential: oauth_credential.credential.clone(),
        };
        let provider_id = resolve_provider_id(&state, &channel).await?;
        let credential_id = if let Some(credential_id) =
            parse_optional_query_value::<i64>(request.query.as_deref(), "credential_id")?
        {
            credential_id
        } else {
            resolve_credential_id(&state, provider_id, &provisional).await?
        };
        resolved_credential_id = Some(credential_id);
        let credential_ref = CredentialRef {
            id: credential_id,
            label: oauth_credential.label.clone(),
            credential: oauth_credential.credential.clone(),
        };
        state.upsert_provider_credential_in_memory(&channel, credential_ref.clone());
        persist_provider_and_credential(&state, &channel, &provider, &credential_ref).await?;
    }
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        resolved_credential_id,
        result.response.request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(result.response.status_code),
            headers: result.response.headers.as_slice(),
            body: Some(result.response.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        resolved_credential_id,
        tracked_http_events.as_slice(),
        result.response.request_meta.as_ref(),
    )
    .await;

    Ok(oauth_callback_response_to_axum(
        result,
        resolved_credential_id,
    ))
}

pub(in crate::routes::provider) async fn upstream_usage(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = state.load_http();
    let spoof_http = matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode))
        .then(|| state.load_spoof_http());
    let now = now_unix_ms();
    let credential_id = parse_optional_query_value::<i64>(query.as_deref(), "credential_id")?;
    let (upstream_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_upstream_usage_with_retry_with_spoof(
                http.as_ref(),
                spoof_http.as_deref(),
                state.credential_states(),
                credential_id,
                now,
            )
            .await
    })
    .await;
    enqueue_credential_status_updates_for_request(state.as_ref(), &channel, &provider, now).await;
    let upstream = match upstream_result {
        Ok(upstream) => upstream,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_credential_id = upstream_error_credential_id(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                err_credential_id,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                err_credential_id,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            state.clone(),
            channel.clone(),
            provider.clone(),
            update,
        )
        .await;
    }

    let payload = upstream
        .into_http_payload()
        .await
        .map_err(HttpError::from)?;
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id,
        upstream_request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(payload.status_code),
            headers: payload.headers.as_slice(),
            body: Some(payload.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id.or(credential_id),
        tracked_http_events.as_slice(),
        upstream_request_meta.as_ref(),
    )
    .await;
    Ok(oauth_response_to_axum(payload))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::{HeaderName, HeaderValue};
    use gproxy_admin::MemoryUserKey;
    use gproxy_provider::{
        BuiltinChannel, BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential,
        ChannelId, ChannelSettings, CredentialPickMode, CredentialRef, LocalTokenizerStore,
        ProviderCredentialState, ProviderDefinition, ProviderDispatchTable, ProviderRegistry,
    };
    use gproxy_storage::{
        CredentialStatusQuery, Scope, SeaOrmStorage, StorageWriteWorkerConfig,
        spawn_storage_write_worker, storage_write_channel,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use wreq::Client as WreqClient;

    use crate::app_state::{AppState, AppStateInit, GlobalSettings};

    use super::*;

    fn build_codex_provider(base_url: &str, oauth_issuer_url: &str) -> ProviderDefinition {
        let channel = ChannelId::Builtin(BuiltinChannel::Codex);
        let mut settings =
            ChannelSettings::Builtin(BuiltinChannelSettings::default_for(BuiltinChannel::Codex));
        if let ChannelSettings::Builtin(BuiltinChannelSettings::Codex(value)) = &mut settings {
            value.base_url = base_url.to_string();
            value.oauth_issuer_url = Some(oauth_issuer_url.to_string());
        }

        let mut builtin_credential = BuiltinChannelCredential::blank_for(BuiltinChannel::Codex);
        if let BuiltinChannelCredential::Codex(value) = &mut builtin_credential {
            value.access_token = String::new();
            value.refresh_token = "rt_test".to_string();
            value.id_token = "id_test".to_string();
            value.user_email = Some("dead@example.com".to_string());
            value.account_id = "acct_test".to_string();
            value.expires_at = 0;
        }

        let credential = CredentialRef {
            id: 1,
            label: Some("codex-user-1".to_string()),
            credential: ChannelCredential::Builtin(builtin_credential),
        };

        ProviderDefinition {
            channel,
            dispatch: ProviderDispatchTable::default(),
            settings,
            credential_pick_mode: CredentialPickMode::RoundRobinWithCache,
            cache_affinity_max_keys: gproxy_provider::DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
            credentials: ProviderCredentialState {
                credentials: vec![credential],
                channel_states: Vec::new(),
            },
        }
    }

    async fn spawn_token_error_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind token error server");
        let address = listener
            .local_addr()
            .expect("token error server local addr");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept token request");
            let mut request = vec![0_u8; 4096];
            let _ = stream.read(&mut request).await;
            let body = serde_json::json!({
                "error": {
                    "message": "Your refresh token has already been used to generate a new access token. Please try signing in again.",
                    "type": "invalid_request_error",
                    "param": null,
                    "code": "refresh_token_reused"
                }
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write token error response");
        });
        format!("http://{}", address)
    }

    async fn wait_for_dead_status(storage: &SeaOrmStorage, credential_id: i64, channel: &str) {
        for _ in 0..20 {
            let rows = storage
                .list_credential_statuses(&CredentialStatusQuery {
                    id: Scope::All,
                    credential_id: Scope::Eq(credential_id),
                    channel: Scope::Eq(channel.to_string()),
                    health_kind: Scope::All,
                    limit: Some(10),
                })
                .await
                .expect("query credential statuses");
            if rows
                .iter()
                .any(|row| row.health_kind.eq_ignore_ascii_case("dead"))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("dead credential status was not persisted in time");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upstream_usage_persists_dead_status_when_codex_refresh_token_is_reused() {
        let storage = Arc::new(
            SeaOrmStorage::connect("sqlite::memory:", None)
                .await
                .expect("connect memory storage"),
        );
        storage.sync().await.expect("sync memory storage");

        let (storage_writes, storage_rx) = storage_write_channel(32);
        let worker = spawn_storage_write_worker(
            storage.clone(),
            storage_rx,
            StorageWriteWorkerConfig {
                aggregate_window: Duration::from_millis(5),
                ..Default::default()
            },
        );

        let oauth_issuer_url = spawn_token_error_server().await;
        let provider = build_codex_provider(
            format!("{}/backend-api/codex", oauth_issuer_url).as_str(),
            oauth_issuer_url.as_str(),
        );
        let channel = provider.channel.clone();
        let credential = provider
            .credentials
            .credentials
            .first()
            .cloned()
            .expect("provider credential");

        let mut registry = ProviderRegistry::default();
        registry.upsert(provider.clone());

        let api_key = "test-provider-key".to_string();
        let state = Arc::new(AppState::new(AppStateInit {
            storage: storage.clone(),
            storage_writes,
            http: Arc::new(WreqClient::new()),
            spoof_http: Arc::new(WreqClient::new()),
            global: GlobalSettings::default(),
            providers: registry,
            tokenizers: Arc::new(LocalTokenizerStore::new(std::path::PathBuf::from("/tmp"))),
            users: Vec::new(),
            keys: HashMap::from([(
                api_key.clone(),
                MemoryUserKey {
                    id: 1,
                    user_id: 1,
                    api_key: api_key.clone(),
                    enabled: true,
                },
            )]),
        }));

        persist_provider_and_credential(state.as_ref(), &channel, &provider, &credential)
            .await
            .expect("persist provider and credential");

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(api_key.as_str()).expect("api key header"),
        );

        let result = upstream_usage(
            State(state.clone()),
            Path("codex".to_string()),
            RawQuery(Some("credential_id=1".to_string())),
            headers,
        )
        .await;
        assert!(
            result.is_err(),
            "usage route should fail on reused refresh token"
        );

        wait_for_dead_status(storage.as_ref(), 1, "codex").await;
        let rows = storage
            .list_credential_statuses(&CredentialStatusQuery {
                id: Scope::All,
                credential_id: Scope::Eq(1),
                channel: Scope::Eq("codex".to_string()),
                health_kind: Scope::All,
                limit: Some(10),
            })
            .await
            .expect("query persisted statuses");
        let row = rows
            .into_iter()
            .find(|row| row.health_kind.eq_ignore_ascii_case("dead"))
            .expect("dead status row");
        let last_error = row.last_error.unwrap_or_default();
        assert!(
            last_error.contains("refresh_token_reused"),
            "expected persisted last_error to mention refresh_token_reused, got: {last_error}"
        );

        drop(state);
        worker.abort();
    }
}

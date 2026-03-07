use super::*;

pub(in crate::routes::provider) async fn openai_realtime_upgrade(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, Some(provider_name), uri, headers, ws).await
}

pub(in crate::routes::provider) async fn openai_responses_upgrade(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, Some(provider_name), uri, headers, ws).await
}

pub(in crate::routes::provider) async fn openai_responses_upgrade_unscoped(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, None, uri, headers, ws).await
}

pub(in crate::routes::provider) async fn openai_realtime_upgrade_with_tail(
    State(state): State<Arc<AppState>>,
    Path((provider_name, _tail)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, Some(provider_name), uri, headers, ws).await
}

pub(in crate::routes::provider) async fn handle_openai_realtime_upgrade(
    state: Arc<AppState>,
    provider_name: Option<String>,
    uri: Uri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    Ok(ws.on_upgrade(move |socket| async move {
        let _ =
            run_openai_websocket_session(state, auth, provider_name, uri, headers, socket).await;
    }))
}

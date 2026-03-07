use super::*;

pub async fn execute_codex_oauth_start(
    client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthResponse, UpstreamError> {
    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);
    let issuer = codex_oauth_issuer(settings, request.query.as_deref());

    let mode = parse_oauth_mode(parse_query_value(request.query.as_deref(), "mode").as_deref());
    let state_id = generate_oauth_state();

    match mode {
        OAuthMode::DeviceAuth => {
            let (user_code, request_meta) =
                request_device_user_code(client, issuer.as_str()).await?;
            oauth_states().insert(
                state_id.clone(),
                OAuthState::DeviceAuth {
                    device_auth_id: user_code.device_auth_id.clone(),
                    user_code: user_code.user_code.clone(),
                    interval_secs: user_code.interval.max(1),
                    created_at_unix_ms: now_unix_ms,
                },
            );

            let verification_uri = format!("{}/codex/device", issuer.trim_end_matches('/'));
            Ok(json_oauth_response_with_meta(
                200,
                json!({
                    "auth_url": verification_uri,
                    "verification_uri": format!("{}/codex/device", issuer.trim_end_matches('/')),
                    "user_code": user_code.user_code,
                    "interval": user_code.interval.max(1),
                    "state": state_id,
                    "mode": "device_auth",
                    "instructions": "Open verification_uri, enter user_code, then call /oauth/callback with state.",
                }),
                Some(request_meta),
            ))
        }
        OAuthMode::AuthorizationCode => {
            let code_verifier = generate_code_verifier();
            let code_challenge = generate_code_challenge(code_verifier.as_str());
            let redirect_uri = parse_query_value(request.query.as_deref(), "redirect_uri")
                .unwrap_or_else(|| DEFAULT_BROWSER_REDIRECT_URI.to_string());
            let scope = parse_query_value(request.query.as_deref(), "scope")
                .unwrap_or_else(|| OAUTH_SCOPE.to_string());
            let originator = parse_query_value(request.query.as_deref(), "originator")
                .unwrap_or_else(|| OAUTH_ORIGINATOR.to_string());
            let allowed_workspace_id =
                parse_query_value(request.query.as_deref(), "allowed_workspace_id");
            let auth_url = build_authorize_url(
                issuer.as_str(),
                redirect_uri.as_str(),
                scope.as_str(),
                originator.as_str(),
                code_challenge.as_str(),
                state_id.as_str(),
                allowed_workspace_id.as_deref(),
            );

            oauth_states().insert(
                state_id.clone(),
                OAuthState::AuthorizationCode {
                    code_verifier,
                    redirect_uri: redirect_uri.clone(),
                    created_at_unix_ms: now_unix_ms,
                },
            );

            Ok(json_oauth_response(
                200,
                json!({
                    "auth_url": auth_url,
                    "state": state_id,
                    "redirect_uri": redirect_uri,
                    "scope": scope,
                    "mode": "authorization_code",
                    "instructions": "Open auth_url, then call /oauth/callback with code/state (or callback_url).",
                }),
            ))
        }
    }
}

pub async fn execute_codex_oauth_callback(
    client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthCallbackResult, UpstreamError> {
    if let Some(error) = parse_query_value(request.query.as_deref(), "error") {
        let detail =
            parse_query_value(request.query.as_deref(), "error_description").unwrap_or(error);
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, detail.as_str()),
            credential: None,
        });
    }

    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);
    let issuer = codex_oauth_issuer(settings, request.query.as_deref());

    let state_param = parse_query_value(request.query.as_deref(), "state").or_else(|| {
        parse_query_value(request.query.as_deref(), "callback_url")
            .and_then(|url| extract_code_state_from_callback_url(url.as_str()).1)
    });

    let (state_id, oauth_state) = if let Some(state) = state_param {
        (
            state.clone(),
            oauth_states()
                .get(state.as_str())
                .map(|entry| entry.clone()),
        )
    } else {
        let states = oauth_states();
        if states.is_empty() {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        }
        if states.len() > 1 {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "ambiguous_state"),
                credential: None,
            });
        }
        let Some(entry) = states.iter().next() else {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        };
        (entry.key().clone(), Some(entry.value().clone()))
    };

    let Some(oauth_state) = oauth_state else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing state"),
            credential: None,
        });
    };

    match oauth_state {
        OAuthState::DeviceAuth {
            device_auth_id,
            user_code,
            interval_secs,
            ..
        } => {
            let poll_status = poll_device_authorization(
                client,
                issuer.as_str(),
                device_auth_id.as_str(),
                user_code.as_str(),
            )
            .await?;
            let (poll_status, poll_request_meta) = poll_status;
            let poll_success = match poll_status {
                DeviceAuthPollStatus::Pending => {
                    let message = format!(
                        "authorization_pending: retry after {}s",
                        interval_secs.max(1)
                    );
                    return Ok(UpstreamOAuthCallbackResult {
                        response: json_oauth_error_with_meta(
                            409,
                            message.as_str(),
                            Some(poll_request_meta),
                        ),
                        credential: None,
                    });
                }
                DeviceAuthPollStatus::Authorized(data) => data,
            };

            oauth_states().remove(state_id.as_str());
            let redirect_uri = format!("{}/deviceauth/callback", issuer.trim_end_matches('/'));
            let (tokens, request_meta) = exchange_code_for_tokens(
                client,
                issuer.as_str(),
                redirect_uri.as_str(),
                poll_success.code_verifier.as_str(),
                poll_success.authorization_code.as_str(),
            )
            .await?;
            build_callback_result(tokens, Some(request_meta))
        }
        OAuthState::AuthorizationCode {
            code_verifier,
            redirect_uri,
            ..
        } => {
            let (code, callback_state) =
                match resolve_manual_code_and_state(request.query.as_deref()) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(UpstreamOAuthCallbackResult {
                            response: json_oauth_error(400, message),
                            credential: None,
                        });
                    }
                };

            if let Some(callback_state) = callback_state
                && callback_state != state_id
            {
                return Ok(UpstreamOAuthCallbackResult {
                    response: json_oauth_error(400, "state_mismatch"),
                    credential: None,
                });
            }

            oauth_states().remove(state_id.as_str());
            let (tokens, request_meta) = exchange_code_for_tokens(
                client,
                issuer.as_str(),
                redirect_uri.as_str(),
                code_verifier.as_str(),
                code.as_str(),
            )
            .await?;
            build_callback_result(tokens, Some(request_meta))
        }
    }
}

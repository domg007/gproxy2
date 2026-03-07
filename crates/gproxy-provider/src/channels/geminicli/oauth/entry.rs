use super::*;

pub async fn execute_geminicli_oauth_start(
    _client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthResponse, UpstreamError> {
    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);

    let mode = match parse_geminicli_oauth_mode(
        parse_query_value(request.query.as_deref(), "mode").as_deref(),
    ) {
        Ok(mode) => mode,
        Err(message) => return Ok(json_oauth_error(400, message)),
    };
    let redirect_uri =
        parse_query_value(request.query.as_deref(), "redirect_uri").unwrap_or_else(|| match mode {
            GeminiCliOAuthMode::UserCode => DEFAULT_MANUAL_REDIRECT_URI.to_string(),
            GeminiCliOAuthMode::AuthorizationCode => {
                DEFAULT_AUTHORIZATION_CODE_REDIRECT_URI.to_string()
            }
        });
    let project_id = parse_query_value(request.query.as_deref(), "project_id");
    let (state, code_verifier, code_challenge) = generate_state_and_pkce();
    let auth_url = build_authorize_url(
        geminicli_oauth_authorize_url(settings),
        redirect_uri.as_str(),
        state.as_str(),
        code_challenge.as_str(),
    );

    oauth_states().insert(
        state.clone(),
        OAuthState {
            code_verifier,
            redirect_uri: redirect_uri.clone(),
            project_id,
            created_at_unix_ms: now_unix_ms,
        },
    );

    let (mode_name, instructions) = match mode {
        GeminiCliOAuthMode::UserCode => (
            "user_code",
            "Open auth_url, copy the authorization code, then call /oauth/callback with code/state (or callback_url).",
        ),
        GeminiCliOAuthMode::AuthorizationCode => (
            "authorization_code",
            "Open auth_url and complete browser authorization, then call /oauth/callback with code/state (or callback_url).",
        ),
    };
    Ok(json_oauth_response(
        200,
        json!({
            "auth_url": auth_url,
            "state": state,
            "redirect_uri": redirect_uri,
            "mode": mode_name,
            "instructions": instructions,
        }),
    ))
}

pub async fn execute_geminicli_oauth_callback(
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

    let callback_mode = match parse_query_value(request.query.as_deref(), "mode") {
        Some(mode) => match parse_geminicli_oauth_mode(Some(mode.as_str())) {
            Ok(value) => Some(value),
            Err(message) => {
                return Ok(UpstreamOAuthCallbackResult {
                    response: json_oauth_error(400, message),
                    credential: None,
                });
            }
        },
        None => None,
    };

    let (code, state_param) =
        match resolve_manual_code_and_state(request.query.as_deref(), callback_mode) {
            Ok(value) => value,
            Err(message) => {
                return Ok(UpstreamOAuthCallbackResult {
                    response: json_oauth_error(400, message),
                    credential: None,
                });
            }
        };

    let oauth_state = if let Some(state) = state_param.as_deref() {
        oauth_states().remove(state).map(|(_, value)| value)
    } else {
        if oauth_states().is_empty() {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        }
        if oauth_states().len() > 1 {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "ambiguous_state"),
                credential: None,
            });
        }
        let Some(entry) = oauth_states().iter().next() else {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        };
        let key = entry.key().clone();
        oauth_states().remove(key.as_str()).map(|(_, value)| value)
    };

    let Some(oauth_state) = oauth_state else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing state"),
            credential: None,
        });
    };

    let (token, token_request_meta) = match exchange_code_for_tokens(
        client,
        geminicli_oauth_token_url(settings),
        code.as_str(),
        oauth_state.redirect_uri.as_str(),
        oauth_state.code_verifier.as_str(),
    )
    .await
    {
        Ok(token) => token,
        Err(err) => {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, err.to_string().as_str()),
                credential: None,
            });
        }
    };

    let Some(access_token) = token
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing access_token",
                Some(token_request_meta.clone()),
            ),
            credential: None,
        });
    };
    let Some(refresh_token) = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing refresh_token",
                Some(token_request_meta.clone()),
            ),
            credential: None,
        });
    };

    let project_hint = oauth_state
        .project_id
        .or_else(|| parse_query_value(request.query.as_deref(), "project_id"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let project_id = match resolve_project_id(
        client,
        access_token.as_str(),
        geminicli_base_url(settings),
        project_hint.as_deref(),
    )
    .await
    {
        Ok(project_id) => project_id,
        Err(failure) => {
            let mut response = failure.into_oauth_response();
            response.request_meta = Some(token_request_meta.clone());
            return Ok(UpstreamOAuthCallbackResult {
                response,
                credential: None,
            });
        }
    };

    let user_email = fetch_user_email(
        client,
        access_token.as_str(),
        geminicli_oauth_userinfo_url(settings),
    )
    .await
    .ok()
    .flatten();
    let expires_at_unix_ms =
        now_unix_ms.saturating_add(token.expires_in.unwrap_or(3600).saturating_mul(1000));

    let credential = UpstreamOAuthCredential {
        label: parse_query_value(request.query.as_deref(), "label"),
        credential: ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(
            GeminiCliCredential {
                access_token: access_token.clone(),
                refresh_token: refresh_token.clone(),
                expires_at: expires_at_unix_ms.min(i64::MAX as u64) as i64,
                project_id: project_id.clone(),
                client_id: CLIENT_ID.to_string(),
                client_secret: CLIENT_SECRET.to_string(),
                user_email: user_email.clone(),
            },
        )),
    };

    Ok(UpstreamOAuthCallbackResult {
        response: json_oauth_response_with_meta(
            200,
            json!({
                "access_token": access_token,
                "refresh_token": refresh_token,
                "project_id": project_id,
                "user_email": user_email,
            }),
            Some(token_request_meta),
        ),
        credential: Some(credential),
    })
}

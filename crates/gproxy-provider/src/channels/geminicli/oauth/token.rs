use super::*;

pub(super) fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn geminicli_base_url(settings: &ChannelSettings) -> &str {
    if settings.base_url().trim().is_empty() {
        DEFAULT_BASE_URL
    } else {
        settings.base_url().trim()
    }
}

pub(super) fn normalize_expires_at_ms(value: i64) -> u64 {
    value.max(0) as u64
}

pub(super) fn access_token_valid(material: &GeminiCliAuthMaterial, now_unix_ms: u64) -> bool {
    !material.access_token.trim().is_empty()
        && material
            .expires_at_unix_ms
            .saturating_sub(TOKEN_REFRESH_SKEW_MS)
            > now_unix_ms
}

pub(super) fn prune_oauth_states(now_unix_ms: u64) {
    oauth_states().retain(|_, value| {
        now_unix_ms.saturating_sub(value.created_at_unix_ms) <= OAUTH_STATE_TTL_MS
    });
}

pub(super) async fn exchange_code_for_tokens(
    client: &WreqClient,
    token_url: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<(TokenResponse, UpstreamRequestMeta), UpstreamError> {
    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("code", code)
            .append_pair("client_id", CLIENT_ID)
            .append_pair("client_secret", CLIENT_SECRET)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("code_verifier", code_verifier)
            .append_pair("grant_type", "authorization_code");
        serializer.finish()
    };
    send_token_request(client, token_url, body.as_str()).await
}

pub(super) async fn refresh_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    material: &GeminiCliAuthMaterial,
    now_unix_ms: u64,
) -> Result<GeminiCliRefreshedToken, GeminiCliTokenRefreshError> {
    let refresh_token = material.refresh_token.trim();
    if refresh_token.is_empty() {
        return Err(GeminiCliTokenRefreshError::InvalidCredential(
            "missing refresh_token".to_string(),
        ));
    }
    if material.client_id.trim().is_empty() || material.client_secret.trim().is_empty() {
        return Err(GeminiCliTokenRefreshError::InvalidCredential(
            "missing client credentials".to_string(),
        ));
    }

    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("refresh_token", refresh_token)
            .append_pair("client_id", material.client_id.as_str())
            .append_pair("client_secret", material.client_secret.as_str())
            .append_pair("grant_type", "refresh_token");
        serializer.finish()
    };
    let (response, _request_meta) =
        send_token_request(client, geminicli_oauth_token_url(settings), body.as_str())
            .await
            .map_err(|err| GeminiCliTokenRefreshError::Transient(err.to_string()))?;

    let access_token = response
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            GeminiCliTokenRefreshError::Transient("token response missing access_token".to_string())
        })?;

    let expires_at_unix_ms = now_unix_ms
        .saturating_add(response.expires_in.unwrap_or(3600).saturating_mul(1000))
        .max(now_unix_ms + 60_000);
    let refresh_token = response
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let user_email = if material
        .user_email
        .as_deref()
        .map(str::trim)
        .is_none_or(|value| value.is_empty())
    {
        fetch_user_email(
            client,
            access_token.as_str(),
            geminicli_oauth_userinfo_url(settings),
        )
        .await
        .ok()
        .flatten()
    } else {
        None
    };

    Ok(GeminiCliRefreshedToken {
        access_token,
        refresh_token,
        expires_at_unix_ms,
        user_email,
    })
}

pub(super) async fn send_token_request(
    client: &WreqClient,
    token_url: &str,
    body: &str,
) -> Result<(TokenResponse, UpstreamRequestMeta), UpstreamError> {
    let headers = vec![(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    )];
    let (response, request_meta) = tracked_send_request(
        client,
        WreqMethod::POST,
        token_url,
        headers,
        Some(body.as_bytes().to_vec()),
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;

    let token = serde_json::from_slice::<TokenResponse>(&bytes)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !status.is_success() {
        let message = token_response_error(status.as_u16(), &token, &bytes);
        return Err(UpstreamError::UpstreamRequest(message));
    }
    Ok((token, request_meta))
}

pub(super) fn token_response_error(
    status_code: u16,
    token: &TokenResponse,
    bytes: &[u8],
) -> String {
    if let Some(error) = token.error.as_deref() {
        let detail = token.error_description.as_deref().unwrap_or_default();
        return format!("oauth token failed: {status_code} {error} {detail}");
    }
    format!(
        "oauth token failed: {status_code} {}",
        String::from_utf8_lossy(bytes)
    )
}

pub(super) fn validation_required_from_payload(
    payload: &serde_json::Value,
) -> Option<ProjectResolutionFailure> {
    let tiers = payload
        .get("ineligibleTiers")
        .and_then(serde_json::Value::as_array)?;
    for tier in tiers {
        let reason_code = tier
            .get("reasonCode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if reason_code != "VALIDATION_REQUIRED" {
            continue;
        }
        let validation_url = tier
            .get("validationUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string();
        let reason = tier
            .get("reasonMessage")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("account validation required")
            .to_string();
        let learn_more_url = tier
            .get("validationLearnMoreUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        return Some(ProjectResolutionFailure::ValidationRequired {
            reason,
            validation_url,
            learn_more_url,
        });
    }
    None
}

pub(super) fn ineligible_reasons_from_payload(payload: &serde_json::Value) -> Vec<String> {
    payload
        .get("ineligibleTiers")
        .and_then(serde_json::Value::as_array)
        .map(|tiers| {
            tiers
                .iter()
                .filter_map(|tier| {
                    tier.get("reasonMessage")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn has_current_tier(payload: &serde_json::Value) -> bool {
    payload
        .get("currentTier")
        .map(|value| !value.is_null())
        .unwrap_or(false)
}

pub(super) fn default_onboard_tier_id(payload: &serde_json::Value) -> String {
    if let Some(tiers) = payload
        .get("allowedTiers")
        .and_then(serde_json::Value::as_array)
    {
        for tier in tiers {
            if tier.get("isDefault").and_then(serde_json::Value::as_bool) == Some(true)
                && let Some(id) = tier
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            {
                return id.to_string();
            }
        }
    }
    "legacy-tier".to_string()
}

pub(super) async fn resolve_project_id(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
    project_id: Option<&str>,
) -> Result<String, ProjectResolutionFailure> {
    let load_payload = load_code_assist_payload(client, access_token, base_url, project_id)
        .await
        .map_err(|err| ProjectResolutionFailure::Upstream(err.to_string()))?;

    if let Some(payload) = load_payload.as_ref() {
        if let Some(validation_required) = validation_required_from_payload(payload) {
            return Err(validation_required);
        }
        let ineligible_reasons = ineligible_reasons_from_payload(payload);

        if has_current_tier(payload) {
            if let Some(project) = payload
                .get("cloudaicompanionProject")
                .and_then(parse_project_id_value)
            {
                return Ok(project);
            }
            if let Some(project) = project_id {
                return Ok(project.to_string());
            }
            if !ineligible_reasons.is_empty() {
                return Err(ProjectResolutionFailure::IneligibleTiers {
                    reasons: ineligible_reasons,
                });
            }
            return Err(ProjectResolutionFailure::MissingProjectId);
        }

        let tier_id = default_onboard_tier_id(payload);
        let onboarded =
            onboard_user_project(client, access_token, base_url, tier_id.as_str(), project_id)
                .await
                .map_err(|err| ProjectResolutionFailure::Upstream(err.to_string()))?;
        if let Some(project) = onboarded {
            return Ok(project);
        }
        if let Some(project) = project_id {
            return Ok(project.to_string());
        }
        if !ineligible_reasons.is_empty() {
            return Err(ProjectResolutionFailure::IneligibleTiers {
                reasons: ineligible_reasons,
            });
        }
        return Err(ProjectResolutionFailure::MissingProjectId);
    }

    let onboarded = onboard_user_project(client, access_token, base_url, "legacy-tier", project_id)
        .await
        .map_err(|err| ProjectResolutionFailure::Upstream(err.to_string()))?;
    if let Some(project) = onboarded {
        return Ok(project);
    }
    if let Some(project) = project_id {
        return Ok(project.to_string());
    }
    Err(ProjectResolutionFailure::MissingProjectId)
}

pub(super) fn code_assist_metadata(project_id: Option<&str>) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("ideType".to_string(), json!("IDE_UNSPECIFIED"));
    metadata.insert("platform".to_string(), json!("PLATFORM_UNSPECIFIED"));
    metadata.insert("pluginType".to_string(), json!("GEMINI"));
    if let Some(project) = project_id.map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert("duetProject".to_string(), json!(project));
    }
    serde_json::Value::Object(metadata)
}

pub(super) fn parse_project_id_value(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

pub(super) async fn load_code_assist_payload(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
    project_id: Option<&str>,
) -> Result<Option<serde_json::Value>, UpstreamError> {
    let url = format!(
        "{}/v1internal:loadCodeAssist",
        base_url.trim_end_matches('/')
    );
    let body = json!({
        "cloudaicompanionProject": project_id,
        "metadata": code_assist_metadata(project_id),
    });
    let user_agent = geminicli_user_agent(None);
    let response = tracked_request(client, WreqMethod::POST, url.as_str())
        .bearer_auth(access_token)
        .header("user-agent", user_agent.as_str())
        .header("accept-encoding", "gzip")
        .header("content-type", "application/json")
        .body(
            serde_json::to_vec(&body)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
        )
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    Ok(Some(payload))
}

pub(super) async fn onboard_user_project(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
    tier_id: &str,
    project_id: Option<&str>,
) -> Result<Option<String>, UpstreamError> {
    let url = format!("{}/v1internal:onboardUser", base_url.trim_end_matches('/'));
    let project_for_request = if tier_id.eq_ignore_ascii_case("free-tier") {
        None
    } else {
        project_id
    };
    let body = json!({
        "tierId": tier_id,
        "cloudaicompanionProject": project_for_request,
        "metadata": code_assist_metadata(project_for_request),
    });
    let body = serde_json::to_vec(&body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let user_agent = geminicli_user_agent(None);
    let response = tracked_request(client, WreqMethod::POST, url.as_str())
        .bearer_auth(access_token)
        .header("user-agent", user_agent.as_str())
        .header("accept-encoding", "gzip")
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let response_bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let mut payload: serde_json::Value = serde_json::from_slice(&response_bytes)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    for _ in 0..5 {
        if payload.get("done").and_then(serde_json::Value::as_bool) == Some(true) {
            let project = payload
                .get("response")
                .and_then(|value| value.get("cloudaicompanionProject"))
                .and_then(parse_project_id_value);
            return Ok(project);
        }
        let Some(name) = payload
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            break;
        };
        let operation_url = format!("{}/v1internal/{name}", base_url.trim_end_matches('/'));
        let poll_response = tracked_request(client, WreqMethod::GET, operation_url.as_str())
            .bearer_auth(access_token)
            .header("user-agent", user_agent.as_str())
            .header("accept-encoding", "gzip")
            .send()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        if !poll_response.status().is_success() {
            break;
        }
        let poll_bytes = poll_response
            .bytes()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        payload = serde_json::from_slice(&poll_bytes)
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    }
    Ok(None)
}

pub(super) fn parse_user_email(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("email")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) async fn fetch_user_email(
    client: &WreqClient,
    access_token: &str,
    userinfo_url: &str,
) -> Result<Option<String>, UpstreamError> {
    let user_agent = geminicli_user_agent(None);
    let response = tracked_request(client, WreqMethod::GET, userinfo_url)
        .bearer_auth(access_token)
        .header("user-agent", user_agent.as_str())
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let payload = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    Ok(parse_user_email(&payload))
}

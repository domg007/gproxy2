use super::*;

pub(crate) fn geminicli_auth_material_from_credential(
    credential: &GeminiCliCredential,
) -> Option<GeminiCliAuthMaterial> {
    let access_token = credential.access_token.trim().to_string();
    let refresh_token = credential.refresh_token.trim().to_string();
    let client_id = credential.client_id.trim().to_string();
    let client_secret = credential.client_secret.trim().to_string();

    if access_token.is_empty() && refresh_token.is_empty() {
        return None;
    }

    Some(GeminiCliAuthMaterial {
        access_token,
        refresh_token,
        client_id: if client_id.is_empty() {
            CLIENT_ID.to_string()
        } else {
            client_id
        },
        client_secret: if client_secret.is_empty() {
            CLIENT_SECRET.to_string()
        } else {
            client_secret
        },
        project_id: credential.project_id.clone(),
        expires_at_unix_ms: normalize_expires_at_ms(credential.expires_at),
        user_email: credential.user_email.clone(),
    })
}

pub async fn ensure_geminicli_project_id(
    client: &WreqClient,
    settings: &ChannelSettings,
    credential: &mut GeminiCliCredential,
) -> Result<(), UpstreamError> {
    if !credential.project_id.trim().is_empty() {
        return Ok(());
    }

    let now_unix_ms = current_unix_ms();
    let Some(material) = geminicli_auth_material_from_credential(credential) else {
        return Err(UpstreamError::SerializeRequest(
            "invalid geminicli credential: missing auth material".to_string(),
        ));
    };

    let resolved = resolve_geminicli_access_token(
        client,
        settings,
        "geminicli::project-detect",
        &material,
        now_unix_ms,
        false,
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.as_message()))?;

    if let Some(refreshed) = resolved.refreshed.as_ref() {
        credential.apply_token_refresh(
            refreshed.access_token.as_str(),
            refreshed.refresh_token.as_deref(),
            refreshed.expires_at_unix_ms,
            refreshed.user_email.as_deref(),
        );
    }

    let project_id = resolve_project_id(
        client,
        resolved.access_token.as_str(),
        geminicli_base_url(settings),
        None,
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.as_message()))?;

    let project_id = project_id.trim().to_string();
    if project_id.is_empty() {
        return Err(UpstreamError::SerializeRequest(
            "missing project_id (auto-detect failed)".to_string(),
        ));
    }
    credential.project_id = project_id;
    Ok(())
}

pub(crate) async fn resolve_geminicli_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    cache_key: &str,
    material: &GeminiCliAuthMaterial,
    now_unix_ms: u64,
    force_refresh: bool,
) -> Result<GeminiCliResolvedAccessToken, GeminiCliTokenRefreshError> {
    if !force_refresh {
        if let Some(cached) = geminicli_token_cache().get(cache_key).filter(|item| {
            item.expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
        }) {
            return Ok(GeminiCliResolvedAccessToken {
                access_token: cached.access_token.clone(),
                refreshed: None,
            });
        }

        if access_token_valid(material, now_unix_ms) {
            geminicli_token_cache().insert(
                cache_key.to_string(),
                CachedGeminiCliToken {
                    access_token: material.access_token.clone(),
                    expires_at_unix_ms: material.expires_at_unix_ms,
                },
            );
            return Ok(GeminiCliResolvedAccessToken {
                access_token: material.access_token.clone(),
                refreshed: None,
            });
        }
    }

    let refreshed = refresh_access_token(client, settings, material, now_unix_ms).await?;
    geminicli_token_cache().insert(
        cache_key.to_string(),
        CachedGeminiCliToken {
            access_token: refreshed.access_token.clone(),
            expires_at_unix_ms: refreshed.expires_at_unix_ms,
        },
    );
    Ok(GeminiCliResolvedAccessToken {
        access_token: refreshed.access_token.clone(),
        refreshed: Some(refreshed),
    })
}

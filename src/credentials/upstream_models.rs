//! On-demand "pull models from the upstream" for a provider: pick an enabled
//! credential, ensure its secret is fresh, send a `list_models` request through
//! the channel (same proxy + TLS identity its traffic uses), and parse the
//! upstream's native model list into `(id, display_name)` rows. Admin-triggered,
//! infrequent — mirrors [`super::usage`].

use std::sync::Arc;

use bytes::Bytes;
use serde::Serialize;
use serde_json::Value;

use crate::app::AppState;
use crate::channel::{Channel, ChannelError, PrepareCtx};
use crate::http::client::UpstreamClient;
use crate::protocol::{Operation, OperationKey, Provider};

/// One model offered by the upstream.
#[derive(Debug, Clone, Serialize)]
pub struct UpstreamModel {
    pub id: String,
    pub display_name: Option<String>,
}

/// Why a model pull could not produce a list.
#[derive(Debug, thiserror::Error)]
pub enum ModelsError {
    #[error("provider not found")]
    ProviderNotFound,
    #[error("provider has no enabled credential")]
    NoCredential,
    #[error("unknown channel: {0}")]
    UnknownChannel(String),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error("decrypt secret: {0}")]
    Decrypt(String),
    #[error("upstream model request failed: {0}")]
    Upstream(String),
    #[error("upstream returned HTTP {0}")]
    Status(u16),
    #[error("{0}")]
    Internal(String),
}

/// Fetch the upstream model list for one provider.
pub async fn fetch_models(
    state: &AppState,
    provider_id: i64,
) -> Result<Vec<UpstreamModel>, ModelsError> {
    let provider = state
        .persistence
        .get_provider(provider_id)
        .await
        .map_err(|e| ModelsError::Internal(e.to_string()))?
        .ok_or(ModelsError::ProviderNotFound)?;
    let channel = state
        .channels
        .get(&provider.channel)
        .ok_or_else(|| ModelsError::UnknownChannel(provider.channel.clone()))?;
    let family = channel.provider_family();

    // Pick an enabled credential — the pull authenticates to the upstream.
    let credential = state
        .persistence
        .list_credentials(provider_id)
        .await
        .map_err(|e| ModelsError::Internal(e.to_string()))?
        .into_iter()
        .find(|c| c.enabled)
        .ok_or(ModelsError::NoCredential)?;

    let opened = state
        .cipher
        .open(&credential.secret_json)
        .map_err(|e| ModelsError::Decrypt(e.to_string()))?;
    let secret = state
        .refresh
        .ensure_fresh(state, &channel, &credential, &provider, opened, false)
        .await?;
    let client = super::usage::resolve_client(state, &channel, &credential, &provider)
        .map_err(|e| ModelsError::Upstream(e.to_string()))?;

    fetch_models_with(&channel, family, &secret, &provider.settings_json, &client).await
}

/// Transport-injectable core: build the `list_models` request, send it, parse.
async fn fetch_models_with(
    channel: &Arc<dyn Channel>,
    family: Provider,
    secret: &Value,
    settings: &Value,
    client: &Arc<dyn UpstreamClient>,
) -> Result<Vec<UpstreamModel>, ModelsError> {
    let target = crate::protocol::request_target(
        OperationKey::provider(Operation::ListModels, family),
        "",
        false,
    );
    let headers = http::HeaderMap::new();
    let prepared = channel.prepare(PrepareCtx {
        secret,
        provider_settings: settings,
        upstream_model_id: "",
        method: http::Method::GET,
        path: &target.path,
        query: target.query.as_deref(),
        headers: &headers,
        body: Bytes::new(),
    })?;

    let resp = client
        .send(prepared.into_http())
        .await
        .map_err(|e| ModelsError::Upstream(e.to_string()))?;
    let status = resp.status();
    let body = resp.into_body();
    if !status.is_success() {
        return Err(ModelsError::Status(status.as_u16()));
    }
    Ok(parse_models(family, &body))
}

/// Parse an upstream native model-list response into `(id, display_name)` rows.
/// openai/claude → `data[]` (`id`); gemini → `models[]` (`name`, `models/` stripped).
fn parse_models(family: Provider, body: &[u8]) -> Vec<UpstreamModel> {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let key = match family {
        Provider::Gemini => "models",
        _ => "data",
    };
    let Some(arr) = v.get(key).and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|m| {
            let id = match family {
                Provider::Gemini => m
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|s| s.strip_prefix("models/").unwrap_or(s).to_owned()),
                _ => m.get("id").and_then(Value::as_str).map(str::to_owned),
            }?;
            let display_name = match family {
                Provider::Gemini => m.get("displayName"),
                Provider::Claude => m.get("display_name"),
                Provider::OpenAi => None,
            }
            .and_then(Value::as_str)
            .map(str::to_owned);
            Some(UpstreamModel { id, display_name })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_openai_and_gemini() {
        let oa = br#"{"object":"list","data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#;
        let ids: Vec<_> = parse_models(Provider::OpenAi, oa)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["gpt-4o", "gpt-4o-mini"]);

        let gm = br#"{"models":[{"name":"models/gemini-1.5-pro","displayName":"Gemini 1.5 Pro"}]}"#;
        let g = parse_models(Provider::Gemini, gm);
        assert_eq!(g[0].id, "gemini-1.5-pro");
        assert_eq!(g[0].display_name.as_deref(), Some("Gemini 1.5 Pro"));
    }
}

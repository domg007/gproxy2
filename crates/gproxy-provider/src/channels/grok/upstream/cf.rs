use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::upstream::tracked_request;

use super::super::settings::GrokSettings;
use super::web::normalize_sso_material;
use super::*;

static GROK_CF_SESSION_CACHE: LazyLock<DashMap<String, GrokCfSession>> =
    LazyLock::new(DashMap::default);

#[derive(Debug, Clone, Default)]
pub(super) struct GrokResolvedSession {
    pub user_agent: Option<String>,
    pub extra_cookie_header: Option<String>,
}

#[derive(Debug, Clone)]
struct GrokCfSession {
    user_agent: Option<String>,
    extra_cookie_header: String,
    fetched_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct FlareSolverrResponse {
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    solution: Option<FlareSolverrSolution>,
}

#[derive(Debug, Deserialize)]
struct FlareSolverrSolution {
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default, rename = "userAgent")]
    user_agent_legacy: Option<String>,
    #[serde(default)]
    cookies: Vec<FlareSolverrCookie>,
}

#[derive(Debug, Deserialize)]
struct FlareSolverrCookie {
    name: String,
    value: String,
}

impl GrokCfSession {
    fn is_fresh(&self, ttl_seconds: u64, now_unix_ms: u64) -> bool {
        now_unix_ms.saturating_sub(self.fetched_at_unix_ms) < ttl_seconds.saturating_mul(1000)
    }

    fn into_resolved(self) -> GrokResolvedSession {
        GrokResolvedSession {
            user_agent: self.user_agent,
            extra_cookie_header: (!self.extra_cookie_header.is_empty())
                .then_some(self.extra_cookie_header),
        }
    }
}

pub(super) async fn resolve_grok_session(
    client: &WreqClient,
    settings: &GrokSettings,
    base_url: &str,
    sso: &str,
) -> Result<GrokResolvedSession, UpstreamError> {
    let Some(solver_url) = settings.cf_solver_url() else {
        return Ok(GrokResolvedSession::default());
    };

    let cache_key = cache_key(base_url, solver_url, sso);
    let now_unix_ms = now_unix_ms();
    if let Some(entry) = GROK_CF_SESSION_CACHE.get(cache_key.as_str()) {
        if entry.is_fresh(settings.cf_session_ttl_seconds(), now_unix_ms) {
            return Ok(entry.clone().into_resolved());
        }
    }

    let session = fetch_grok_cf_session(client, settings, base_url, sso).await?;
    tracing::info!(
        grok_base_url = %base_url,
        grok_cf_solver_url = %solver_url,
        grok_cf_cookie_len = session.extra_cookie_header.len(),
        grok_cf_has_user_agent = session.user_agent.is_some(),
        "grok cf session refreshed"
    );
    GROK_CF_SESSION_CACHE.insert(cache_key, session.clone());
    Ok(session.into_resolved())
}

pub(super) fn invalidate_grok_session(settings: &GrokSettings, base_url: &str, sso: &str) {
    let Some(solver_url) = settings.cf_solver_url() else {
        return;
    };
    GROK_CF_SESSION_CACHE.remove(cache_key(base_url, solver_url, sso).as_str());
}

async fn fetch_grok_cf_session(
    client: &WreqClient,
    settings: &GrokSettings,
    base_url: &str,
    sso: &str,
) -> Result<GrokCfSession, UpstreamError> {
    let solver_endpoint = solver_endpoint(settings.cf_solver_url().unwrap_or_default());
    let target_url = solver_target_url(base_url);
    let payload = build_solver_payload(target_url.as_str(), sso, settings.cf_solver_timeout_seconds());
    let request_body = serde_json::to_vec(&payload)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let response = tracked_request(client, WreqMethod::POST, solver_endpoint.as_str())
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .body(request_body)
        .send()
        .await
        .map_err(|err| {
            UpstreamError::UpstreamRequest(format!("grok cf solver request failed: {err}"))
        })?;

    let status = response.status();
    let response_body = response
        .bytes()
        .await
        .map_err(|err| {
            UpstreamError::UpstreamRequest(format!("grok cf solver read body failed: {err}"))
        })?;

    if !status.is_success() {
        let body_text = String::from_utf8_lossy(&response_body);
        return Err(UpstreamError::UpstreamRequest(format!(
            "grok cf solver http status {status}: {body_text}"
        )));
    }

    let payload = serde_json::from_slice::<FlareSolverrResponse>(&response_body).map_err(|err| {
        UpstreamError::UpstreamRequest(format!("grok cf solver invalid json response: {err}"))
    })?;
    if !payload.status.trim().eq_ignore_ascii_case("ok") {
        let message = payload
            .message
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown solver error");
        return Err(UpstreamError::UpstreamRequest(format!(
            "grok cf solver returned {}: {message}",
            payload.status
        )));
    }

    let solution = payload.solution.ok_or_else(|| {
        UpstreamError::UpstreamRequest("grok cf solver returned no solution".to_string())
    })?;
    let extra_cookie_header = cookies_from_solver(solution.cookies.as_slice())?;
    let user_agent = solution
        .user_agent
        .or(solution.user_agent_legacy)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(GrokCfSession {
        user_agent,
        extra_cookie_header,
        fetched_at_unix_ms: now_unix_ms(),
    })
}

fn build_solver_payload(target_url: &str, sso: &str, timeout_seconds: u64) -> Value {
    let mut payload = json!({
        "cmd": "request.get",
        "url": target_url,
        "maxTimeout": timeout_seconds.saturating_mul(1000),
        "waitInSeconds": 3,
        "disableMedia": true,
    });

    if let Some(token) = normalize_sso_material(sso) {
        payload["cookies"] = json!([
            { "name": "sso", "value": token },
            { "name": "sso-rw", "value": token }
        ]);
    }

    payload
}

fn cookies_from_solver(cookies: &[FlareSolverrCookie]) -> Result<String, UpstreamError> {
    let value = cookies
        .iter()
        .filter_map(|cookie| {
            let name = cookie.name.trim();
            let value = cookie.value.trim();
            if name.is_empty() || value.is_empty() {
                return None;
            }
            if name.eq_ignore_ascii_case("sso") || name.eq_ignore_ascii_case("sso-rw") {
                return None;
            }
            Some(format!("{name}={value}"))
        })
        .collect::<Vec<_>>()
        .join("; ");

    if value.is_empty() {
        return Err(UpstreamError::UpstreamRequest(
            "grok cf solver returned no reusable cookies".to_string(),
        ));
    }

    Ok(value)
}

fn solver_endpoint(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn solver_target_url(base_url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(base_url) else {
        return "https://grok.com/".to_string();
    };
    parsed.set_path("/");
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

fn cache_key(base_url: &str, solver_url: &str, sso: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(normalize_sso_material(sso).unwrap_or_default().as_bytes());
    let sso_hash = format!("{:x}", digest.finalize());
    format!(
        "{}|{}|{}",
        base_url.trim().to_ascii_lowercase(),
        solver_url.trim().to_ascii_lowercase(),
        sso_hash
    )
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{cache_key, cookies_from_solver, solver_endpoint, solver_target_url};

    #[test]
    fn solver_endpoint_appends_v1_once() {
        assert_eq!(solver_endpoint("http://127.0.0.1:8191"), "http://127.0.0.1:8191/v1");
        assert_eq!(solver_endpoint("http://127.0.0.1:8191/v1"), "http://127.0.0.1:8191/v1");
    }

    #[test]
    fn solver_target_url_normalizes_to_origin_root() {
        assert_eq!(solver_target_url("https://grok.com/rest/app-chat"), "https://grok.com/");
    }

    #[test]
    fn cache_key_changes_with_sso() {
        assert_ne!(
            cache_key("https://grok.com", "http://solver/v1", "one"),
            cache_key("https://grok.com", "http://solver/v1", "two")
        );
    }

    #[test]
    fn cookies_from_solver_drops_sso_pairs() {
        let cookies = vec![
            super::FlareSolverrCookie {
                name: "sso".to_string(),
                value: "a".to_string(),
            },
            super::FlareSolverrCookie {
                name: "__cf_bm".to_string(),
                value: "b".to_string(),
            },
            super::FlareSolverrCookie {
                name: "cf_clearance".to_string(),
                value: "c".to_string(),
            },
        ];
        assert_eq!(
            cookies_from_solver(cookies.as_slice()).expect("cookies"),
            "__cf_bm=b; cf_clearance=c"
        );
    }
}

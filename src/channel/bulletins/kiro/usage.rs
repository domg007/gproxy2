//! Kiro / Amazon Q per-credential usage — `GET {base}/getUsageLimits`. Returns
//! the subscription title plus a usage breakdown (current vs. limit, in
//! fractional "agentic request" units) with a unix reset. Mirrors the request
//! the Kiro IDE issues (`origin=AI_EDITOR&resourceType=AGENTIC_REQUEST`, the
//! optional `profileArn`, and the AWS SDK invocation headers).

use bytes::Bytes;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use http::{HeaderMap, Method, Request, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use super::{DEFAULT_BASE_URL, USER_AGENT_VALUE, auth};
use crate::channel::ChannelError;
use crate::channel::http_util::{build_request, join_url};
use crate::channel::usage::{UsageSnapshot, UsageWindow};

/// Build `GET {base}/getUsageLimits?...` with the AWS SDK fingerprint headers.
pub(super) fn request(
    secret: &Value,
    settings: &Value,
) -> Result<Option<Request<Bytes>>, ChannelError> {
    let access_token = auth::access_token(secret)?;
    let base = settings
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_BASE_URL);

    let mut path = format!(
        "/getUsageLimits?origin={}&resourceType=AGENTIC_REQUEST",
        super::ORIGIN
    );
    if let Some(arn) = auth::profile_arn(secret, settings) {
        path.push_str("&profileArn=");
        path.push_str(&pct(arn));
    }

    let uri = join_url(base, &path, None)?;
    let mut req = build_request(Method::GET, uri, HeaderMap::new(), Bytes::new())?;
    let bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|e| ChannelError::InvalidCredential(format!("bad access_token: {e}")))?;
    let invocation = HeaderValue::from_str(&crate::util::rand::uuid_v4())
        .map_err(|e| ChannelError::Build(format!("bad invocation id: {e}")))?;
    let h = req.headers_mut();
    h.insert(AUTHORIZATION, bearer);
    h.insert(ACCEPT, HeaderValue::from_static("application/json"));
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    h.insert(HeaderName::from_static("amz-sdk-invocation-id"), invocation);
    h.insert(
        HeaderName::from_static("amz-sdk-request"),
        HeaderValue::from_static("attempt=1; max=1"),
    );
    Ok(Some(req))
}

/// Parse the `getUsageLimits` body (`UsageLimitsResponse`) into a snapshot.
pub(super) fn parse(status: StatusCode, body: &Bytes) -> Option<UsageSnapshot> {
    if !status.is_success() {
        return None;
    }
    let raw: Value = serde_json::from_slice(body).ok()?;
    let resp: UsageLimitsResponse = serde_json::from_value(raw.clone()).ok()?;

    let single = resp.usage_breakdown_list.len() == 1;
    let windows = resp
        .usage_breakdown_list
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let name = if single {
                "agentic_request".to_string()
            } else {
                format!("agentic_request_{i}")
            };
            let mut w = UsageWindow::amounts(
                name,
                b.current_usage_with_precision.unwrap_or(0.0),
                b.usage_limit_with_precision.unwrap_or(0.0),
            );
            if let Some(reset) = b.next_date_reset.or(resp.next_date_reset) {
                w = w.resets_unix(reset as i64);
            }
            w
        })
        .collect();

    Some(UsageSnapshot {
        plan: resp
            .subscription_info
            .and_then(|s| s.subscription_title)
            .filter(|s| !s.is_empty()),
        windows,
        credits: None,
        raw,
    })
}

/// Percent-encode a query value, leaving the RFC 3986 unreserved set verbatim
/// (`profileArn` carries `:` and `/`).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(
                char::from_digit((b >> 4) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit((b & 0xf) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
        }
    }
    out
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageLimitsResponse {
    #[serde(default)]
    next_date_reset: Option<f64>,
    #[serde(default)]
    subscription_info: Option<SubscriptionInfo>,
    #[serde(default)]
    usage_breakdown_list: Vec<UsageBreakdown>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionInfo {
    #[serde(default)]
    subscription_title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBreakdown {
    #[serde(default)]
    current_usage_with_precision: Option<f64>,
    #[serde(default)]
    usage_limit_with_precision: Option<f64>,
    #[serde(default)]
    next_date_reset: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_limits() {
        let body = Bytes::from_static(
            br#"{
              "nextDateReset": 1735689600,
              "subscriptionInfo": {"subscriptionTitle": "KIRO PRO+"},
              "usageBreakdownList": [
                {"currentUsage": 120, "currentUsageWithPrecision": 120.5,
                 "usageLimit": 1000, "usageLimitWithPrecision": 1000.0, "nextDateReset": 1735689600}
              ]
            }"#,
        );
        let snap = parse(StatusCode::OK, &body).expect("snapshot");
        assert_eq!(snap.plan.as_deref(), Some("KIRO PRO+"));
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].name, "agentic_request");
        assert_eq!(snap.windows[0].used, Some(120.5));
        assert_eq!(snap.windows[0].limit, Some(1000.0));
        assert_eq!(snap.windows[0].resets_at_unix, Some(1735689600));
    }
}

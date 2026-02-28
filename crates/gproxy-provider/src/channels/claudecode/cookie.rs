use base64::Engine as _;
use rand::Rng as _;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use url::form_urlencoded;
use wreq::header::{HeaderMap, HeaderValue};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{CLAUDE_API_VERSION, CLIENT_ID, OAUTH_BETA, OAUTH_SCOPE, TOKEN_UA};
use super::oauth::parse_query_value as parse_simple_query_value;

#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in: Option<u64>,
    #[serde(default, alias = "subscriptionType")]
    pub(crate) subscription_type: Option<String>,
    #[serde(default, alias = "rateLimitTier")]
    pub(crate) rate_limit_tier: Option<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapResponse {
    #[serde(default)]
    account: BootstrapAccount,
}

#[derive(Debug, Default, Deserialize)]
struct BootstrapAccount {
    #[serde(default)]
    memberships: Vec<BootstrapMembership>,
}

#[derive(Debug, Default, Deserialize)]
struct BootstrapMembership {
    #[serde(default)]
    organization: BootstrapOrg,
}

#[derive(Debug, Default, Deserialize)]
struct BootstrapOrg {
    uuid: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OrganizationEntry {
    uuid: Option<String>,
    id: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

pub(crate) async fn exchange_tokens_with_cookie(
    client: &WreqClient,
    api_base_url: &str,
    claude_ai_base_url: &str,
    redirect_uri: &str,
    cookie: &str,
) -> Result<TokenResponse, String> {
    let org_uuid = fetch_org_uuid(client, cookie, claude_ai_base_url).await?;
    let (code, code_verifier, state) = authorize_with_cookie(
        client,
        cookie,
        claude_ai_base_url,
        api_base_url,
        redirect_uri,
        org_uuid.as_str(),
    )
    .await?;

    let cleaned_code = code.split('#').next().unwrap_or(code.as_str());
    let cleaned_code = cleaned_code.split('&').next().unwrap_or(cleaned_code);

    let mut body = format!(
        "grant_type=authorization_code&client_id={}&code={}&redirect_uri={}&code_verifier={}",
        url_encode(CLIENT_ID),
        url_encode(cleaned_code),
        url_encode(redirect_uri),
        url_encode(code_verifier.as_str()),
    );
    body.push_str("&state=");
    body.push_str(url_encode(state.as_str()).as_str());

    let origin = claude_ai_base_url.trim_end_matches('/');
    let response = client
        .request(
            WreqMethod::POST,
            format!("{}/v1/oauth/token", api_base_url.trim_end_matches('/')).as_str(),
        )
        .header("anthropic-version", CLAUDE_API_VERSION)
        .header("anthropic-beta", OAUTH_BETA)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json, text/plain, */*")
        .header("user-agent", TOKEN_UA)
        .header("origin", origin)
        .header("referer", format!("{origin}/"))
        .body(body)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(format!("oauth_token_failed: {status} {text}"));
    }

    serde_json::from_slice::<TokenResponse>(&bytes).map_err(|err| err.to_string())
}

async fn fetch_org_uuid(
    client: &WreqClient,
    cookie: &str,
    claude_ai_base_url: &str,
) -> Result<String, String> {
    if let Ok(uuid) = fetch_org_uuid_from_bootstrap(client, cookie, claude_ai_base_url).await {
        return Ok(uuid);
    }
    fetch_org_uuid_from_organizations(client, cookie, claude_ai_base_url).await
}

async fn fetch_org_uuid_from_bootstrap(
    client: &WreqClient,
    cookie: &str,
    claude_ai_base_url: &str,
) -> Result<String, String> {
    let response = client
        .request(
            WreqMethod::GET,
            format!("{}/api/bootstrap", claude_ai_base_url.trim_end_matches('/')).as_str(),
        )
        .headers(build_cookie_headers(cookie, claude_ai_base_url)?)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("cookie_org_bootstrap_failed: {status}"));
    }

    let payload =
        serde_json::from_slice::<BootstrapResponse>(&bytes).map_err(|err| err.to_string())?;
    for membership in payload.account.memberships {
        if !membership
            .organization
            .capabilities
            .iter()
            .any(|value| value == "chat")
        {
            continue;
        }

        if let Some(uuid) = membership.organization.uuid {
            return Ok(uuid);
        }
    }

    Err("no bootstrap organization with chat capability".to_string())
}

async fn fetch_org_uuid_from_organizations(
    client: &WreqClient,
    cookie: &str,
    claude_ai_base_url: &str,
) -> Result<String, String> {
    let response = client
        .request(
            WreqMethod::GET,
            format!(
                "{}/api/organizations",
                claude_ai_base_url.trim_end_matches('/')
            )
            .as_str(),
        )
        .headers(build_cookie_headers(cookie, claude_ai_base_url)?)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("cookie_org_failed: {status}"));
    }

    let payload =
        serde_json::from_slice::<Vec<OrganizationEntry>>(&bytes).map_err(|err| err.to_string())?;
    for entry in payload {
        if !entry.capabilities.iter().any(|value| value == "chat") {
            continue;
        }
        if let Some(uuid) = entry.uuid.or(entry.id) {
            return Ok(uuid);
        }
    }

    Err("no organization with chat capability".to_string())
}

async fn authorize_with_cookie(
    client: &WreqClient,
    cookie: &str,
    claude_ai_base_url: &str,
    api_base_url: &str,
    redirect_uri: &str,
    org_uuid: &str,
) -> Result<(String, String, String), String> {
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(code_verifier.as_str());
    let state = generate_state();

    let payload = json!({
        "response_type": "code",
        "client_id": CLIENT_ID,
        "organization_uuid": org_uuid,
        "redirect_uri": redirect_uri,
        "scope": OAUTH_SCOPE,
        "state": state,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
    });

    let mut headers = build_cookie_headers(cookie, claude_ai_base_url)?;
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let response = client
        .request(
            WreqMethod::POST,
            format!(
                "{}/v1/oauth/{}/authorize",
                api_base_url.trim_end_matches('/'),
                org_uuid
            )
            .as_str(),
        )
        .headers(headers)
        .body(serde_json::to_vec(&payload).map_err(|err| err.to_string())?)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(format!(
            "cookie_authorize_failed: status={} body={}",
            status, text
        ));
    }

    let payload =
        serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|err| err.to_string())?;
    let redirect = payload
        .get("redirect_uri")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "missing redirect_uri".to_string())?;

    let code = parse_simple_query_value(
        redirect
            .split_once('?')
            .map(|(_, query)| query)
            .or(Some(redirect)),
        "code",
    )
    .ok_or_else(|| "missing code in redirect_uri".to_string())?;

    Ok((code, code_verifier, state))
}

fn build_cookie_headers(cookie: &str, claude_ai_base_url: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert("accept", HeaderValue::from_static("application/json"));
    headers.insert(
        "accept-language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    headers.insert(
        "cookie",
        HeaderValue::from_str(format!("sessionKey={cookie}").as_str())
            .map_err(|err| err.to_string())?,
    );
    let origin = claude_ai_base_url.trim_end_matches('/');
    headers.insert(
        "origin",
        HeaderValue::from_str(origin).map_err(|err| err.to_string())?,
    );
    headers.insert(
        "referer",
        HeaderValue::from_str(format!("{origin}/new").as_str()).map_err(|err| err.to_string())?,
    );
    Ok(headers)
}

fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn url_encode(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
}

use gproxy_provider_core::credential::{
    AntigravityCredential, ClaudeCodeCredential, CodexCredential,
};
use gproxy_provider_core::{Credential, HttpMethod, ProviderConfig, UpstreamCtx, header_get};

use gproxy_provider_core::config::{AntigravityConfig, ClaudeCodeConfig, CodexConfig};

use gproxy_provider_impl::register_builtin_providers;

#[tokio::test]
async fn codex_upstream_usage_request_shape() {
    let mut reg = gproxy_provider_core::ProviderRegistry::new();
    register_builtin_providers(&mut reg);
    let provider = reg.get("codex").unwrap();

    let config = ProviderConfig::Codex(CodexConfig {
        base_url: Some("https://chatgpt.com/backend-api/codex".to_string()),
    });
    let cred = Credential::Codex(CodexCredential {
        access_token: "t".to_string(),
        refresh_token: "rtok".to_string(),
        id_token: "idtok".to_string(),
        user_email: None,
        account_id: "acc".to_string(),
        expires_at: 0,
    });

    let ctx = UpstreamCtx {
        trace_id: None,
        user_id: None,
        user_key_id: None,
        user_agent: None,
        outbound_proxy: None,
        provider: "codex".to_string(),
        credential_id: Some(1),
        op: gproxy_provider_core::Op::GenerateContent,
        internal: true,
        attempt_no: 0,
    };

    let req = provider
        .build_upstream_usage(&ctx, &config, &cred)
        .await
        .unwrap();
    assert_eq!(req.method, HttpMethod::Get);
    assert_eq!(req.url, "https://chatgpt.com/backend-api/wham/usage");
    assert_eq!(header_get(&req.headers, "authorization"), Some("Bearer t"));
    assert_eq!(header_get(&req.headers, "chatgpt-account-id"), Some("acc"));
    assert_eq!(header_get(&req.headers, "accept"), Some("application/json"));
}

#[tokio::test]
async fn claudecode_upstream_usage_request_shape() {
    let mut reg = gproxy_provider_core::ProviderRegistry::new();
    register_builtin_providers(&mut reg);
    let provider = reg.get("claudecode").unwrap();

    let config = ProviderConfig::ClaudeCode(ClaudeCodeConfig {
        base_url: None,
        claude_ai_base_url: None,
        platform_base_url: Some("https://console.anthropic.com/".to_string()),
        prelude_text: None,
    });
    let cred = Credential::ClaudeCode(ClaudeCodeCredential {
        access_token: "t".to_string(),
        refresh_token: "rtok".to_string(),
        expires_at: 0,
        enable_claude_1m_sonnet: None,
        enable_claude_1m_opus: None,
        supports_claude_1m_sonnet: None,
        supports_claude_1m_opus: None,
        subscription_type: String::new(),
        rate_limit_tier: String::new(),
        user_email: None,
        session_key: None,
    });

    let ctx = UpstreamCtx {
        trace_id: None,
        user_id: None,
        user_key_id: None,
        user_agent: None,
        outbound_proxy: None,
        provider: "claudecode".to_string(),
        credential_id: Some(2),
        op: gproxy_provider_core::Op::GenerateContent,
        internal: true,
        attempt_no: 0,
    };

    let req = provider
        .build_upstream_usage(&ctx, &config, &cred)
        .await
        .unwrap();
    assert_eq!(req.method, HttpMethod::Get);
    assert_eq!(req.url, "https://console.anthropic.com/api/oauth/usage");
    assert_eq!(header_get(&req.headers, "authorization"), Some("Bearer t"));
    assert_eq!(
        header_get(&req.headers, "anthropic-beta"),
        Some("oauth-2025-04-20")
    );
    assert_eq!(
        header_get(&req.headers, "user-agent"),
        Some("claude-code/2.1.27")
    );
}

#[tokio::test]
async fn antigravity_upstream_usage_request_shape() {
    let mut reg = gproxy_provider_core::ProviderRegistry::new();
    register_builtin_providers(&mut reg);
    let provider = reg.get("antigravity").unwrap();

    let config = ProviderConfig::Antigravity(AntigravityConfig {
        base_url: Some("https://daily-cloudcode-pa.sandbox.googleapis.com/".to_string()),
    });
    let cred = Credential::Antigravity(AntigravityCredential {
        access_token: "t".to_string(),
        refresh_token: "rtok".to_string(),
        expires_at: 0,
        project_id: "proj".to_string(),
        client_id: "cid".to_string(),
        client_secret: "csecret".to_string(),
        user_email: None,
    });

    let ctx = UpstreamCtx {
        trace_id: None,
        user_id: None,
        user_key_id: None,
        user_agent: None,
        outbound_proxy: None,
        provider: "antigravity".to_string(),
        credential_id: Some(3),
        op: gproxy_provider_core::Op::GenerateContent,
        internal: true,
        attempt_no: 0,
    };

    let req = provider
        .build_upstream_usage(&ctx, &config, &cred)
        .await
        .unwrap();
    assert_eq!(req.method, HttpMethod::Post);
    assert_eq!(
        req.url,
        "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels"
    );
    assert_eq!(header_get(&req.headers, "authorization"), Some("Bearer t"));
    assert_eq!(
        header_get(&req.headers, "content-type"),
        Some("application/json")
    );
    assert!(req.body.is_some());
}

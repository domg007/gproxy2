pub const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_BROWSER_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const OAUTH_SCOPE: &str = "openid profile email offline_access";
pub const OAUTH_ORIGINATOR: &str = "codex_vscode";
pub const OAUTH_STATE_TTL_MS: u64 = 600_000;
pub const TOKEN_REFRESH_SKEW_MS: u64 = 60_000;

pub const CLIENT_VERSION: &str = "0.110.0";
pub const ACCOUNT_ID_HEADER: &str = "chatgpt-account-id";
pub const ORIGINATOR_HEADER: &str = "originator";
pub const ORIGINATOR_VALUE: &str = "codex_vscode";
pub const USER_AGENT_HEADER: &str = "user-agent";
pub const USER_AGENT_VALUE: &str = "codex_vscode/0.110.0";

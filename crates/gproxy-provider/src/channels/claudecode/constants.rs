pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_CLAUDE_AI_BASE_URL: &str = "https://claude.ai";
pub const DEFAULT_PLATFORM_BASE_URL: &str = "https://platform.claude.com";

pub const DEFAULT_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";

pub const CLAUDE_CODE_VERSION: &str = "2.1.76";
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const OAUTH_SCOPE: &str = "user:profile user:inference user:sessions:claude_code";
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
pub const CLAUDE_API_VERSION: &str = "2023-06-01";
pub const CLAUDE_CODE_BILLING_HEADER_PREFIX: &str = "x-anthropic-billing-header:";
pub const CLAUDE_CODE_BILLING_ENTRYPOINT: &str = "cli";
pub const CLAUDE_CODE_BILLING_SALT: &str = "59cf53e54c78";
pub const CLAUDE_CODE_BILLING_CCH: &str = "00000";

pub const TOKEN_UA: &str = "claude-cli/2.1.76 (external, cli)";
pub const CLAUDE_CODE_UA: &str = "claude-code/2.1.76";

pub const OAUTH_STATE_TTL_MS: u64 = 600_000;
pub const TOKEN_REFRESH_SKEW_MS: u64 = 60_000;

#[cfg(test)]
pub const CLAUDECODE_REFERENCE_BETAS: &[&str] = &[
    "message-batches-2024-09-24",
    "prompt-caching-2024-07-31",
    "computer-use-2024-10-22",
    "computer-use-2025-01-24",
    "pdfs-2024-09-25",
    "token-counting-2024-11-01",
    "token-efficient-tools-2025-02-19",
    "output-128k-2025-02-19",
    "files-api-2025-04-14",
    "mcp-client-2025-04-04",
    "mcp-client-2025-11-20",
    "dev-full-thinking-2025-05-14",
    "interleaved-thinking-2025-05-14",
    "code-execution-2025-05-22",
    "extended-cache-ttl-2025-04-11",
    "context-1m-2025-08-07",
    "context-management-2025-06-27",
    "model-context-window-exceeded-2025-08-26",
    "skills-2025-10-02",
    "fast-mode-2026-02-01",
    "claude-code-20250219",
    "adaptive-thinking-2026-01-28",
    "prompt-caching-scope-2026-01-05",
    "advanced-tool-use-2025-11-20",
    "effort-2025-11-24",
];

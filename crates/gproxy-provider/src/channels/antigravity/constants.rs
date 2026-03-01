pub const DEFAULT_BASE_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
pub const ANTIGRAVITY_USER_AGENT: &str = "antigravity/1.15.8 (Windows; AMD64)";

pub const DEFAULT_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const DEFAULT_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const DEFAULT_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";

pub const CLIENT_ID: &str = "REPLACE_WITH_GOOGLE_OAUTH_CLIENT_ID.apps.googleusercontent.com";
pub const CLIENT_SECRET: &str = "REPLACE_WITH_GOOGLE_OAUTH_CLIENT_SECRET";
pub const OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";

pub const OAUTH_STATE_TTL_MS: u64 = 600_000;
pub const TOKEN_REFRESH_SKEW_MS: u64 = 60_000;

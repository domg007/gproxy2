import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://cloudcode-pa.googleapis.com",
  "user_agent": "GeminiCLI/0.30.0/gemini-2.5-pro (os; arch)",
  "oauth_authorize_url": "https://accounts.google.com/o/oauth2/v2/auth",
  "oauth_token_url": "https://oauth2.googleapis.com/token",
  "oauth_userinfo_url": "https://www.googleapis.com/oauth2/v2/userinfo"
} as const;
const OPTIONAL_KEYS = [
  "user_agent",
  "oauth_authorize_url",
  "oauth_token_url",
  "oauth_userinfo_url"
] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

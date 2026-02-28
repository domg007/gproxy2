import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://daily-cloudcode-pa.sandbox.googleapis.com",
  "oauth_authorize_url": "https://accounts.google.com/o/oauth2/v2/auth",
  "oauth_token_url": "https://oauth2.googleapis.com/token",
  "oauth_userinfo_url": "https://www.googleapis.com/oauth2/v1/userinfo?alt=json"
} as const;
const OPTIONAL_KEYS = ["oauth_authorize_url", "oauth_token_url", "oauth_userinfo_url"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

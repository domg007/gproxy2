import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://aiplatform.googleapis.com",
  "oauth_token_url": "https://oauth2.googleapis.com/token"
} as const;
const OPTIONAL_KEYS = ["oauth_token_url"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

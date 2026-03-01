import { DEFAULT_GPROXY_USER_AGENT_DRAFT, createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://aiplatform.googleapis.com",
  "user_agent": DEFAULT_GPROXY_USER_AGENT_DRAFT,
  "oauth_token_url": "https://oauth2.googleapis.com/token"
} as const;
const OPTIONAL_KEYS = ["user_agent", "oauth_token_url"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://chatgpt.com/backend-api/codex",
  "oauth_issuer_url": "https://auth.openai.com"
} as const;
const OPTIONAL_KEYS = ["oauth_issuer_url"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

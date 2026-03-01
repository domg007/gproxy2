import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://chatgpt.com/backend-api/codex",
  "user_agent": "codex_vscode/0.99.0",
  "oauth_issuer_url": "https://auth.openai.com"
} as const;
const OPTIONAL_KEYS = ["user_agent", "oauth_issuer_url"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

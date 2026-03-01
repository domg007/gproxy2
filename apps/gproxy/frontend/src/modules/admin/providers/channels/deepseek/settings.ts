import { DEFAULT_GPROXY_USER_AGENT_DRAFT, createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://api.deepseek.com",
  "user_agent": DEFAULT_GPROXY_USER_AGENT_DRAFT
} as const;
const OPTIONAL_KEYS = ["user_agent"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

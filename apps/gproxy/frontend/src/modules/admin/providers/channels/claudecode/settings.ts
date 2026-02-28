import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://api.anthropic.com",
  "claudecode_ai_base_url": "https://claude.ai",
  "claudecode_platform_base_url": "https://platform.claude.com",
  "claudecode_prelude_text": ""
} as const;
const OPTIONAL_KEYS = ["claudecode_ai_base_url", "claudecode_platform_base_url", "claudecode_prelude_text"] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

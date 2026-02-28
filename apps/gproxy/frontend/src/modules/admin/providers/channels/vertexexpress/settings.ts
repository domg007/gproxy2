import { createSettingsCodec } from "../shared";

const DEFAULTS = {
  "base_url": "https://aiplatform.googleapis.com"
} as const;
const OPTIONAL_KEYS = [] as const;

export const {
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson
} = createSettingsCodec(DEFAULTS as unknown as Record<string, string>, [...OPTIONAL_KEYS]);

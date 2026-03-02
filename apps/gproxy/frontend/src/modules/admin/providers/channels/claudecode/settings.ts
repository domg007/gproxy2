import type { ChannelSettingsDraft } from "../../types";
import {
  normalizeTopLevelCacheControlModeDraft,
  topLevelCacheControlModeDraftToSettingsValue,
} from "../shared";

const DEFAULTS = {
  base_url: "https://api.anthropic.com",
  user_agent: "claude-code/2.1.62",
  claudecode_ai_base_url: "https://claude.ai",
  claudecode_platform_base_url: "https://platform.claude.com",
  claudecode_prelude_text: "",
  enable_top_level_cache_control: "off"
} as const;

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

export function defaultSettingsDraft(): ChannelSettingsDraft {
  return { ...DEFAULTS };
}

export function parseSettingsDraft(value: unknown): ChannelSettingsDraft {
  if (!isObject(value)) {
    return defaultSettingsDraft();
  }

  const out: ChannelSettingsDraft = { ...DEFAULTS };
  if (typeof value.base_url === "string") {
    out.base_url = value.base_url;
  }
  if (typeof value.user_agent === "string") {
    out.user_agent = value.user_agent;
  }
  if (typeof value.claudecode_ai_base_url === "string") {
    out.claudecode_ai_base_url = value.claudecode_ai_base_url;
  }
  if (typeof value.claudecode_platform_base_url === "string") {
    out.claudecode_platform_base_url = value.claudecode_platform_base_url;
  }
  if (typeof value.claudecode_prelude_text === "string") {
    out.claudecode_prelude_text = value.claudecode_prelude_text;
  }
  if ("enable_top_level_cache_control" in value) {
    out.enable_top_level_cache_control = normalizeTopLevelCacheControlModeDraft(
      value.enable_top_level_cache_control
    );
  }
  return out;
}

export function buildSettingsJson(settings: ChannelSettingsDraft): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    base_url: (settings.base_url ?? DEFAULTS.base_url).trim()
  };

  const userAgent = (settings.user_agent ?? DEFAULTS.user_agent).trim();
  const defaultUserAgent = DEFAULTS.user_agent.trim();
  if (userAgent !== defaultUserAgent) {
    payload.user_agent = userAgent;
  }

  const aiBaseUrl = (settings.claudecode_ai_base_url ?? DEFAULTS.claudecode_ai_base_url).trim();
  if (aiBaseUrl && aiBaseUrl !== DEFAULTS.claudecode_ai_base_url.trim()) {
    payload.claudecode_ai_base_url = aiBaseUrl;
  }

  const platformBaseUrl = (
    settings.claudecode_platform_base_url ?? DEFAULTS.claudecode_platform_base_url
  ).trim();
  if (platformBaseUrl && platformBaseUrl !== DEFAULTS.claudecode_platform_base_url.trim()) {
    payload.claudecode_platform_base_url = platformBaseUrl;
  }

  const preludeText = (settings.claudecode_prelude_text ?? DEFAULTS.claudecode_prelude_text).trim();
  if (preludeText) {
    payload.claudecode_prelude_text = preludeText;
  }

  const cacheControlMode = normalizeTopLevelCacheControlModeDraft(
    settings.enable_top_level_cache_control ?? DEFAULTS.enable_top_level_cache_control
  );
  const cacheControlModeValue = topLevelCacheControlModeDraftToSettingsValue(cacheControlMode);
  if (cacheControlModeValue) {
    payload.enable_top_level_cache_control = cacheControlModeValue;
  }

  return payload;
}

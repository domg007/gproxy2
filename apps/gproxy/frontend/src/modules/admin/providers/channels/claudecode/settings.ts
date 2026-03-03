import type { ChannelSettingsDraft } from "../../types";
import {
  cacheBreakpointRulesDraftToSettingsValue,
  cacheBreakpointRulesDraftToStoredString,
  normalizeCacheBreakpointRulesDraft,
} from "../shared";

const DEFAULTS = {
  base_url: "https://api.anthropic.com",
  user_agent: "claude-code/2.1.62",
  claudecode_ai_base_url: "https://claude.ai",
  claudecode_platform_base_url: "https://platform.claude.com",
  claudecode_prelude_text: "",
  cache_breakpoints: "[]"
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
  if ("cache_breakpoints" in value) {
    out.cache_breakpoints = cacheBreakpointRulesDraftToStoredString(
      normalizeCacheBreakpointRulesDraft(value.cache_breakpoints)
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

  const cacheBreakpointRules = cacheBreakpointRulesDraftToSettingsValue(
    settings.cache_breakpoints ?? DEFAULTS.cache_breakpoints
  );
  if (cacheBreakpointRules.length > 0) {
    payload.cache_breakpoints = cacheBreakpointRules;
  }

  return payload;
}

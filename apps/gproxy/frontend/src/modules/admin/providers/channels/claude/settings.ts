import type { ChannelSettingsDraft } from "../../types";
import { DEFAULT_GPROXY_USER_AGENT_DRAFT } from "../shared";
import {
  cacheBreakpointRulesDraftToSettingsValue,
  cacheBreakpointRulesDraftToStoredString,
  normalizeCacheBreakpointRulesDraft,
} from "../shared";

const DEFAULTS = {
  base_url: "https://api.anthropic.com",
  user_agent: DEFAULT_GPROXY_USER_AGENT_DRAFT,
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

  const cacheBreakpointRules = cacheBreakpointRulesDraftToSettingsValue(
    settings.cache_breakpoints ?? DEFAULTS.cache_breakpoints
  );
  if (cacheBreakpointRules.length > 0) {
    payload.cache_breakpoints = cacheBreakpointRules;
  }

  return payload;
}

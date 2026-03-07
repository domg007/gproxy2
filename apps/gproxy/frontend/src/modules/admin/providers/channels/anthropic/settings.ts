import type { ChannelSettingsDraft } from "../../types";
import {
  anthropicExtraBetaHeadersDraftToList,
  anthropicExtraBetaHeadersDraftToString
} from "../claudecode/settings";
import { DEFAULT_GPROXY_USER_AGENT_DRAFT } from "../shared";
import {
  cacheBreakpointRulesDraftToSettingsValue,
  cacheBreakpointRulesDraftToStoredString,
  normalizeCacheBreakpointRulesDraft,
} from "../shared";

const DEFAULTS = {
  base_url: "https://api.anthropic.com",
  user_agent: DEFAULT_GPROXY_USER_AGENT_DRAFT,
  anthropic_append_beta_query: "false",
  anthropic_prelude_text: "",
  anthropic_extra_beta_headers: "",
  cache_breakpoints: "[]"
} as const;

function normalizeBooleanDraft(value: unknown, fallback: "true" | "false" = "false"): "true" | "false" {
  if (typeof value === "boolean") {
    return value ? "true" : "false";
  }
  if (typeof value === "string") {
    return value.trim().toLowerCase() === "true" ? "true" : "false";
  }
  return fallback;
}

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
  if ("anthropic_append_beta_query" in value) {
    out.anthropic_append_beta_query = normalizeBooleanDraft(value.anthropic_append_beta_query);
  }
  if (typeof value.anthropic_prelude_text === "string") {
    out.anthropic_prelude_text = value.anthropic_prelude_text;
  }
  if ("anthropic_extra_beta_headers" in value) {
    out.anthropic_extra_beta_headers = anthropicExtraBetaHeadersDraftToString(
      value.anthropic_extra_beta_headers
    );
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

  if ((settings.anthropic_append_beta_query ?? DEFAULTS.anthropic_append_beta_query) === "true") {
    payload.anthropic_append_beta_query = true;
  }

  const preludeText = (settings.anthropic_prelude_text ?? DEFAULTS.anthropic_prelude_text).trim();
  if (preludeText) {
    payload.anthropic_prelude_text = preludeText;
  }

  const extraBetaHeaders = anthropicExtraBetaHeadersDraftToList(
    settings.anthropic_extra_beta_headers ?? DEFAULTS.anthropic_extra_beta_headers
  );
  if (extraBetaHeaders.length > 0) {
    payload.anthropic_extra_beta_headers = extraBetaHeaders;
  }

  const cacheBreakpointRules = cacheBreakpointRulesDraftToSettingsValue(
    settings.cache_breakpoints ?? DEFAULTS.cache_breakpoints
  );
  if (cacheBreakpointRules.length > 0) {
    payload.cache_breakpoints = cacheBreakpointRules;
  }

  return payload;
}

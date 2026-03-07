import type { ChannelSettingsDraft } from "../../types";
import {
  cacheBreakpointRulesDraftToSettingsValue,
  cacheBreakpointRulesDraftToStoredString,
  normalizeCacheBreakpointRulesDraft,
} from "../shared";

export const CLAUDECODE_OAUTH_BETA_HEADER = "oauth-2025-04-20";
export const CLAUDECODE_REFERENCE_BETA_HEADERS = [
  "message-batches-2024-09-24",
  "prompt-caching-2024-07-31",
  "computer-use-2024-10-22",
  "computer-use-2025-01-24",
  "pdfs-2024-09-25",
  "token-counting-2024-11-01",
  "token-efficient-tools-2025-02-19",
  "output-128k-2025-02-19",
  "files-api-2025-04-14",
  "mcp-client-2025-04-04",
  "mcp-client-2025-11-20",
  "dev-full-thinking-2025-05-14",
  "interleaved-thinking-2025-05-14",
  "code-execution-2025-05-22",
  "extended-cache-ttl-2025-04-11",
  "context-1m-2025-08-07",
  "context-management-2025-06-27",
  "model-context-window-exceeded-2025-08-26",
  "skills-2025-10-02",
  "fast-mode-2026-02-01",
  "claude-code-20250219",
  "adaptive-thinking-2026-01-28",
  "prompt-caching-scope-2026-01-05",
  "advanced-tool-use-2025-11-20",
  "effort-2025-11-24"
] as const;

const DEFAULTS = {
  base_url: "https://api.anthropic.com",
  user_agent: "claude-code/2.1.62",
  claudecode_ai_base_url: "https://claude.ai",
  claudecode_platform_base_url: "https://platform.claude.com",
  claudecode_prelude_text: "",
  claudecode_extra_beta_headers: "",
  cache_breakpoints: "[]"
} as const;


function normalizeExtraBetaHeaders(value: unknown): string[] {
  const rawValues = Array.isArray(value)
    ? value
    : typeof value === "string"
      ? value.split(",")
      : [];

  const out: string[] = [];
  for (const item of rawValues) {
    if (typeof item !== "string") {
      continue;
    }
    const trimmed = item.trim();
    if (!trimmed || trimmed.toLowerCase() === CLAUDECODE_OAUTH_BETA_HEADER.toLowerCase()) {
      continue;
    }
    if (!out.some((existing) => existing.toLowerCase() === trimmed.toLowerCase())) {
      out.push(trimmed);
    }
  }
  return out;
}

export function claudecodeExtraBetaHeadersDraftToList(value: unknown): string[] {
  return normalizeExtraBetaHeaders(value);
}

export function claudecodeExtraBetaHeadersDraftToString(value: unknown): string {
  return normalizeExtraBetaHeaders(value).join(", ");
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
  if (typeof value.claudecode_ai_base_url === "string") {
    out.claudecode_ai_base_url = value.claudecode_ai_base_url;
  }
  if (typeof value.claudecode_platform_base_url === "string") {
    out.claudecode_platform_base_url = value.claudecode_platform_base_url;
  }
  if (typeof value.claudecode_prelude_text === "string") {
    out.claudecode_prelude_text = value.claudecode_prelude_text;
  }
  if ("claudecode_extra_beta_headers" in value) {
    out.claudecode_extra_beta_headers = claudecodeExtraBetaHeadersDraftToString(
      value.claudecode_extra_beta_headers
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

  const extraBetaHeaders = claudecodeExtraBetaHeadersDraftToList(
    settings.claudecode_extra_beta_headers ?? DEFAULTS.claudecode_extra_beta_headers
  );
  if (extraBetaHeaders.length > 0) {
    payload.claudecode_extra_beta_headers = extraBetaHeaders;
  }

  const cacheBreakpointRules = cacheBreakpointRulesDraftToSettingsValue(
    settings.cache_breakpoints ?? DEFAULTS.cache_breakpoints
  );
  if (cacheBreakpointRules.length > 0) {
    payload.cache_breakpoints = cacheBreakpointRules;
  }

  return payload;
}

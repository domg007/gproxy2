import type { ChannelSettingsDraft } from "../../types";

export const DEFAULT_GROK_WEB_USER_AGENT =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

const DEFAULTS = {
  base_url: "https://grok.com",
  user_agent: DEFAULT_GROK_WEB_USER_AGENT,
  cf_solver_url: "",
  cf_solver_timeout_seconds: "60",
  cf_session_ttl_seconds: "1800",
  temporary: "false",
  disable_memory: "false"
} as const;

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function normalizeBooleanDraft(value: unknown, fallback: "true" | "false" = "false"): "true" | "false" {
  if (typeof value === "boolean") {
    return value ? "true" : "false";
  }
  if (typeof value === "string") {
    return value.trim().toLowerCase() === "true" ? "true" : "false";
  }
  return fallback;
}

function normalizeIntegerDraft(value: unknown, fallback: string): string {
  if (typeof value === "number" && Number.isInteger(value) && value > 0) {
    return String(value);
  }
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (trimmed) {
      const parsed = Number(trimmed);
      if (Number.isInteger(parsed) && parsed > 0) {
        return String(parsed);
      }
    }
  }
  return fallback;
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
  if (typeof value.cf_solver_url === "string") {
    out.cf_solver_url = value.cf_solver_url;
  }
  if ("cf_solver_timeout_seconds" in value) {
    out.cf_solver_timeout_seconds = normalizeIntegerDraft(
      value.cf_solver_timeout_seconds,
      DEFAULTS.cf_solver_timeout_seconds
    );
  }
  if ("cf_session_ttl_seconds" in value) {
    out.cf_session_ttl_seconds = normalizeIntegerDraft(
      value.cf_session_ttl_seconds,
      DEFAULTS.cf_session_ttl_seconds
    );
  }
  if ("temporary" in value) {
    out.temporary = normalizeBooleanDraft(value.temporary);
  }
  if ("disable_memory" in value) {
    out.disable_memory = normalizeBooleanDraft(value.disable_memory);
  }
  return out;
}

export function buildSettingsJson(settings: ChannelSettingsDraft): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    base_url: (settings.base_url ?? DEFAULTS.base_url).trim()
  };

  const userAgent = (settings.user_agent ?? DEFAULTS.user_agent).trim();
  if (userAgent && userAgent !== DEFAULTS.user_agent) {
    payload.user_agent = userAgent;
  }

  const cfSolverUrl = (settings.cf_solver_url ?? DEFAULTS.cf_solver_url).trim();
  if (cfSolverUrl) {
    payload.cf_solver_url = cfSolverUrl;
  }

  const cfSolverTimeout = normalizeIntegerDraft(
    settings.cf_solver_timeout_seconds,
    DEFAULTS.cf_solver_timeout_seconds
  );
  if (cfSolverTimeout !== DEFAULTS.cf_solver_timeout_seconds) {
    payload.cf_solver_timeout_seconds = Number(cfSolverTimeout);
  }

  const cfSessionTtl = normalizeIntegerDraft(
    settings.cf_session_ttl_seconds,
    DEFAULTS.cf_session_ttl_seconds
  );
  if (cfSessionTtl !== DEFAULTS.cf_session_ttl_seconds) {
    payload.cf_session_ttl_seconds = Number(cfSessionTtl);
  }

  if ((settings.temporary ?? DEFAULTS.temporary) === "true") {
    payload.temporary = true;
  }
  if ((settings.disable_memory ?? DEFAULTS.disable_memory) === "true") {
    payload.disable_memory = true;
  }

  return payload;
}

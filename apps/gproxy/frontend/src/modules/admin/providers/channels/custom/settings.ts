import type { ChannelSettingsDraft } from "../../types";
import { DEFAULT_GPROXY_USER_AGENT_DRAFT } from "../shared";

const DEFAULTS = {
  base_url: "",
  user_agent: DEFAULT_GPROXY_USER_AGENT_DRAFT,
  mask_table: ""
} as const;

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function toJsonObject(value: unknown): Record<string, unknown> | null {
  if (isObject(value)) {
    return value;
  }
  if (typeof value === "string") {
    try {
      const parsed = JSON.parse(value);
      return isObject(parsed) ? parsed : null;
    } catch {
      return null;
    }
  }
  return null;
}

export function defaultSettingsDraft(): ChannelSettingsDraft {
  return { ...DEFAULTS };
}

export function parseSettingsDraft(value: unknown): ChannelSettingsDraft {
  const root = toJsonObject(value);
  if (!root) {
    return defaultSettingsDraft();
  }
  const out: ChannelSettingsDraft = { ...DEFAULTS };
  if (typeof root.base_url === "string") {
    out.base_url = root.base_url;
  }
  if (typeof root.user_agent === "string") {
    out.user_agent = root.user_agent;
  }

  const rawMaskTable = root.mask_table;
  if (typeof rawMaskTable === "string") {
    out.mask_table = rawMaskTable;
  } else if (rawMaskTable != null) {
    try {
      out.mask_table = JSON.stringify(rawMaskTable, null, 2);
    } catch {
      out.mask_table = "";
    }
  }
  return out;
}

export function buildSettingsJson(settings: ChannelSettingsDraft): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    base_url: (settings.base_url ?? DEFAULTS.base_url).trim()
  };
  const userAgent = (settings.user_agent ?? DEFAULTS.user_agent).trim();
  if (userAgent) {
    payload.user_agent = userAgent;
  }
  const rawMaskTable = settings.mask_table ?? DEFAULTS.mask_table;
  const trimmed = rawMaskTable.trim();
  if (trimmed) {
    try {
      payload.mask_table = JSON.parse(trimmed);
    } catch {
      payload.mask_table = trimmed;
    }
  }
  return payload;
}

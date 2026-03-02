import type { ChannelSettingsDraft } from "../types";

export const BUILD_UA_OS = __APP_OS__;
export const BUILD_UA_ARCH = __APP_ARCH__;
export const DEFAULT_GPROXY_USER_AGENT_DRAFT = `gproxy/${__APP_VERSION__}(${BUILD_UA_OS},${BUILD_UA_ARCH})`;
export type TopLevelCacheControlModeDraft = "off" | "auto" | "5m" | "1h";

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

export function normalizeTopLevelCacheControlModeDraft(
  value: unknown
): TopLevelCacheControlModeDraft {
  if (typeof value !== "string") {
    return "off";
  }
  switch (value.trim().toLowerCase()) {
    case "auto":
      return "auto";
    case "5m":
      return "5m";
    case "1h":
      return "1h";
    case "off":
      return "off";
    default:
      return "off";
  }
}

export function topLevelCacheControlModeDraftToSettingsValue(
  value: TopLevelCacheControlModeDraft
): string | null {
  if (value === "off") {
    return null;
  }
  if (value === "auto") {
    return "auto";
  }
  return value;
}

export function createSettingsCodec(defaults: Record<string, string>, optionalKeys: string[]) {
  const defaultSettingsDraft = (): ChannelSettingsDraft => ({ ...defaults });

  const parseSettingsDraft = (value: unknown): ChannelSettingsDraft => {
    const root = toJsonObject(value);
    if (!root) {
      return defaultSettingsDraft();
    }
    const out: ChannelSettingsDraft = { ...defaults };
    for (const key of Object.keys(defaults)) {
      const raw = root[key];
      if (typeof raw === "string") {
        out[key] = raw;
      }
    }
    return out;
  };

  const buildSettingsJson = (settings: ChannelSettingsDraft): Record<string, unknown> => {
    const payload: Record<string, unknown> = {
      base_url: (settings.base_url ?? defaults.base_url ?? "").trim()
    };
    for (const key of optionalKeys) {
      const value = (settings[key] ?? "").trim();
      const def = defaults[key] ?? "";
      if (key === "user_agent") {
        if (value !== def) {
          payload[key] = value;
        }
        continue;
      }
      if (value && value !== def) {
        payload[key] = value;
      }
    }
    return payload;
  };

  return { defaultSettingsDraft, parseSettingsDraft, buildSettingsJson };
}

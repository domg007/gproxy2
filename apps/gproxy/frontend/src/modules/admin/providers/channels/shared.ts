import type { ChannelSettingsDraft } from "../types";

export const BUILD_UA_OS = __APP_OS__;
export const BUILD_UA_ARCH = __APP_ARCH__;
export const DEFAULT_GPROXY_USER_AGENT_DRAFT = `gproxy/${__APP_VERSION__}(${BUILD_UA_OS},${BUILD_UA_ARCH})`;
export type CacheBreakpointTargetDraft = "top_level" | "tools" | "system" | "messages";
export type CacheBreakpointPositionDraft = "nth" | "last_nth";
export type CacheBreakpointTtlDraft = "auto" | "5m" | "1h";
export type CacheBreakpointRuleDraft = {
  target: CacheBreakpointTargetDraft;
  position: CacheBreakpointPositionDraft;
  index: number;
  content_position?: CacheBreakpointPositionDraft;
  content_index?: number;
  ttl: CacheBreakpointTtlDraft;
};

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

const CACHE_BREAKPOINT_TARGET_ORDER: Record<CacheBreakpointTargetDraft, number> = {
  top_level: 0,
  tools: 1,
  system: 2,
  messages: 3
};

function normalizeCacheBreakpointTarget(value: unknown): CacheBreakpointTargetDraft {
  if (typeof value !== "string") {
    return "messages";
  }
  switch (value.trim().toLowerCase()) {
    case "global":
    case "top_level":
      return "top_level";
    case "tools":
      return "tools";
    case "system":
      return "system";
    case "messages":
      return "messages";
    default:
      return "messages";
  }
}

function normalizeCacheBreakpointPosition(value: unknown): CacheBreakpointPositionDraft {
  if (typeof value !== "string") {
    return "nth";
  }
  switch (value.trim().toLowerCase()) {
    case "last":
    case "last_nth":
    case "from_end":
      return "last_nth";
    case "nth":
    default:
      return "nth";
  }
}

function normalizeCacheBreakpointTtl(value: unknown): CacheBreakpointTtlDraft {
  if (typeof value !== "string") {
    return "auto";
  }
  switch (value.trim().toLowerCase()) {
    case "5m":
    case "ttl5m":
      return "5m";
    case "1h":
    case "ttl1h":
      return "1h";
    case "auto":
    default:
      return "auto";
  }
}

function normalizeCacheBreakpointIndex(value: unknown): number {
  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.max(1, Math.trunc(value));
  }
  if (typeof value === "string") {
    const parsed = Number(value.trim());
    if (Number.isFinite(parsed)) {
      return Math.max(1, Math.trunc(parsed));
    }
  }
  return 1;
}

function sortCacheBreakpointRules(rules: CacheBreakpointRuleDraft[]): CacheBreakpointRuleDraft[] {
  return [...rules].sort((a, b) => {
    const targetCmp = CACHE_BREAKPOINT_TARGET_ORDER[a.target] - CACHE_BREAKPOINT_TARGET_ORDER[b.target];
    if (targetCmp !== 0) {
      return targetCmp;
    }
    const posA = a.position === "nth" ? 0 : 1;
    const posB = b.position === "nth" ? 0 : 1;
    if (posA !== posB) {
      return posA - posB;
    }
    if (a.position === "nth") {
      return a.index - b.index;
    }
    return b.index - a.index;
  });
}

function hasMessageContentSelector(rule: CacheBreakpointRuleDraft): boolean {
  return rule.target === "messages" && (rule.content_position !== undefined || rule.content_index !== undefined);
}

export function normalizeCacheBreakpointRulesDraft(value: unknown): CacheBreakpointRuleDraft[] {
  let source: unknown = value;
  if (typeof value === "string") {
    try {
      source = JSON.parse(value);
    } catch {
      return [];
    }
  }
  if (!Array.isArray(source)) {
    return [];
  }
  const normalized = source
    .filter((item): item is Record<string, unknown> => isObject(item))
    .map((item) => {
      const target = normalizeCacheBreakpointTarget(item.target);
      const position = normalizeCacheBreakpointPosition(item.position);
      const index = normalizeCacheBreakpointIndex(item.index);
      const contentSelectorEnabled =
        target === "messages" &&
        ("content_position" in item || "content_index" in item);
      const content_position = contentSelectorEnabled
        ? normalizeCacheBreakpointPosition(item.content_position)
        : undefined;
      const content_index = contentSelectorEnabled
        ? normalizeCacheBreakpointIndex(item.content_index)
        : undefined;
      const ttl = normalizeCacheBreakpointTtl(item.ttl);
      return {
        target,
        position,
        index,
        content_position,
        content_index,
        ttl
      } satisfies CacheBreakpointRuleDraft;
    });
  return sortCacheBreakpointRules(normalized).slice(0, 4);
}

export function cacheBreakpointRulesDraftToStoredString(value: unknown): string {
  return JSON.stringify(normalizeCacheBreakpointRulesDraft(value));
}

export function cacheBreakpointRulesDraftToSettingsValue(value: unknown): Array<Record<string, unknown>> {
  const rules = normalizeCacheBreakpointRulesDraft(value);
  return rules.map((rule) => {
    const base: Record<string, unknown> = {
      target: rule.target,
      ttl: rule.ttl
    };
    if (rule.target !== "top_level") {
      base.position = rule.position;
      base.index = rule.index;
    }
    if (hasMessageContentSelector(rule)) {
      base.content_position = rule.content_position ?? "nth";
      base.content_index = rule.content_index ?? 1;
    }
    return base;
  });
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

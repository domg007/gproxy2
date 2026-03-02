import { getChannelConfig } from "./channels/registry";
import type { ChannelSettingsDraft, CredentialPickMode } from "./types";

const DEFAULT_CREDENTIAL_PICK_MODE: CredentialPickMode = "round_robin_with_cache";

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

export function parseCredentialPickModeFromSettings(
  value: unknown
): CredentialPickMode {
  if (!isObject(value)) {
    return DEFAULT_CREDENTIAL_PICK_MODE;
  }
  const raw = value.credential_pick_mode;
  if (typeof raw === "string") {
    const trimmed = raw.trim();
    if (
      trimmed === "sticky_no_cache" ||
      trimmed === "sticky_with_cache" ||
      trimmed === "round_robin_with_cache" ||
      trimmed === "round_robin_no_cache"
    ) {
      return trimmed;
    }
  }

  const legacyBool = value.cache_affinity_enabled;
  if (typeof legacyBool === "boolean") {
    return legacyBool ? "round_robin_with_cache" : "sticky_no_cache";
  }
  return DEFAULT_CREDENTIAL_PICK_MODE;
}

export function normalizeChannel(channel: string): string {
  return channel.trim().toLowerCase();
}

export function defaultChannelSettingsDraft(channel: string): ChannelSettingsDraft {
  const normalized = normalizeChannel(channel);
  const config = getChannelConfig(normalized) ?? getChannelConfig("custom");
  if (!config) {
    return { base_url: "" };
  }
  return config.defaultSettingsDraft();
}

export function parseChannelSettingsDraft(
  channel: string,
  value: unknown
): ChannelSettingsDraft {
  const normalized = normalizeChannel(channel);
  const config = getChannelConfig(normalized) ?? getChannelConfig("custom");
  if (!config) {
    return { base_url: "" };
  }
  return config.parseSettingsDraft(value);
}

export function buildChannelSettingsJson(
  channel: string,
  settings: ChannelSettingsDraft
): Record<string, unknown> {
  const normalized = normalizeChannel(channel);
  const config = getChannelConfig(normalized) ?? getChannelConfig("custom");
  if (!config) {
    return { base_url: (settings.base_url ?? "").trim() };
  }
  return config.buildSettingsJson(settings);
}

import { getChannelConfig } from "./channels/registry";
import type { ChannelSettingsDraft } from "./types";

type CredentialRoutingFlags = {
  roundRobinEnabled: boolean;
  cacheAffinityEnabled: boolean;
};

const DEFAULT_CREDENTIAL_ROUTING_FLAGS: CredentialRoutingFlags = {
  roundRobinEnabled: true,
  cacheAffinityEnabled: true
};

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function normalizeCredentialRoutingFlags(
  flags: CredentialRoutingFlags
): CredentialRoutingFlags {
  if (!flags.roundRobinEnabled) {
    return {
      roundRobinEnabled: false,
      cacheAffinityEnabled: false
    };
  }
  return flags;
}

export function parseCredentialRoutingFlagsFromSettings(
  value: unknown
): CredentialRoutingFlags {
  if (!isObject(value)) {
    return DEFAULT_CREDENTIAL_ROUTING_FLAGS;
  }
  const roundRobinEnabled = value.credential_round_robin_enabled;
  const cacheAffinityEnabled = value.credential_cache_affinity_enabled;
  if (
    typeof roundRobinEnabled === "boolean" ||
    typeof cacheAffinityEnabled === "boolean"
  ) {
    return normalizeCredentialRoutingFlags({
      roundRobinEnabled:
        typeof roundRobinEnabled === "boolean"
          ? roundRobinEnabled
          : DEFAULT_CREDENTIAL_ROUTING_FLAGS.roundRobinEnabled,
      cacheAffinityEnabled:
        typeof cacheAffinityEnabled === "boolean"
          ? cacheAffinityEnabled
          : DEFAULT_CREDENTIAL_ROUTING_FLAGS.cacheAffinityEnabled
    });
  }

  const raw = value.credential_pick_mode;
  if (typeof raw === "string") {
    const trimmed = raw.trim();
    if (trimmed === "round_robin_with_cache") {
      return {
        roundRobinEnabled: true,
        cacheAffinityEnabled: true
      };
    }
    if (trimmed === "round_robin_no_cache") {
      return {
        roundRobinEnabled: true,
        cacheAffinityEnabled: false
      };
    }
    if (trimmed === "sticky_no_cache" || trimmed === "sticky_with_cache") {
      return {
        roundRobinEnabled: false,
        cacheAffinityEnabled: false
      };
    }
  }

  const legacyBool = value.cache_affinity_enabled;
  if (typeof legacyBool === "boolean") {
    return {
      roundRobinEnabled: legacyBool,
      cacheAffinityEnabled: legacyBool
    };
  }
  return DEFAULT_CREDENTIAL_ROUTING_FLAGS;
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

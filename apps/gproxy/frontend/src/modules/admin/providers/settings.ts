import { getChannelConfig } from "./channels/registry";
import type { ChannelSettingsDraft } from "./types";

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

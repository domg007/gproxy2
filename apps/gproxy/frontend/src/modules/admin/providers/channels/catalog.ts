import type { ProviderChannelCatalogRow } from "../../../../lib/types";

import { BUILTIN_CHANNELS, CHANNEL_CONFIGS } from "./registry";

function normalizeChannelId(channel: string): string {
  return channel.trim().toLowerCase();
}

function catalogByChannel(
  channelCatalog?: ProviderChannelCatalogRow[] | null
): Map<string, ProviderChannelCatalogRow> {
  return new Map(
    (channelCatalog ?? []).map((row) => [normalizeChannelId(row.channel), row])
  );
}

export function buildChannelSelectOptions(
  channelCatalog?: ProviderChannelCatalogRow[] | null
): Array<{ value: string; label: string }> {
  const catalog = catalogByChannel(channelCatalog);
  const builtinChannels =
    catalog.size > 0
      ? Array.from(catalog.keys()).filter(
          (channel) => channel !== "custom" && channel in CHANNEL_CONFIGS
        )
      : BUILTIN_CHANNELS;
  return [
    { value: "custom", label: "custom" },
    ...builtinChannels.map((channel) => ({ value: channel, label: channel }))
  ];
}

export function channelSupportsOAuth(
  channel: string,
  channelCatalog?: ProviderChannelCatalogRow[] | null
): boolean {
  const catalog = catalogByChannel(channelCatalog);
  return catalog.get(normalizeChannelId(channel))?.supports_oauth ?? false;
}

export function channelSupportsUpstreamUsage(
  channel: string,
  channelCatalog?: ProviderChannelCatalogRow[] | null
): boolean {
  const catalog = catalogByChannel(channelCatalog);
  return catalog.get(normalizeChannelId(channel))?.supports_upstream_usage ?? false;
}

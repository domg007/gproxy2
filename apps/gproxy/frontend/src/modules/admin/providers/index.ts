import type { ProviderQueryRow } from "../../../lib/types";
import { getChannelConfig } from "./channels/registry";
import {
  buildCredentialSecretJson,
  credentialDefaultNameFromSecretJson,
  credentialDefaultNameFromSecretValues,
  createEmptyCredentialFormState,
  credentialFormFromRow,
  credentialSchemaForChannel,
  secretValuesFromSecretJson
} from "./credentials";
import {
  availableBulkModes,
  buildBulkExportText,
  defaultBulkMode,
  parseBulkCredentialText
} from "./bulk";
import {
  buildDispatchJson,
  createDefaultDispatchRule,
  defaultDispatchRulesForChannel,
  normalizeDispatchRules,
  resolveProviderDispatchRules
} from "./dispatch";
import {
  buildUsageDisplayRows,
  buildUsageWindowSpecs,
  formatUsagePercent,
  parseLiveUsageRows,
  type UsageDisplayKind,
  type UsageDisplayRow,
  type UsageSampleRow,
  type LiveUsageRow,
  type UsageWindow,
  type UsageWindowSpec
} from "./usage";
import {
  CHANNEL_SELECT_OPTIONS,
  CLAUDE_AGENT_SDK_PRELUDE_TEXT,
  CLAUDE_CODE_SYSTEM_PRELUDE_TEXT,
  LIVE_USAGE_CHANNELS,
  OAUTH_CHANNELS,
  OPERATION_OPTIONS,
  PROTOCOL_OPTIONS,
  isCustomChannel
} from "./constants";
import {
  buildChannelSettingsJson,
  defaultChannelSettingsDraft,
  normalizeChannel,
  parseCredentialRoutingFlagsFromSettings,
  parseChannelSettingsDraft
} from "./settings";
import type {
  BulkCredentialImportEntry,
  ChannelCredentialSchema,
  CredentialBulkMode,
  CredentialFieldSchema,
  CredentialFieldType,
  CredentialFieldValue,
  CredentialsSubTab,
  CredentialFormState,
  DispatchMode,
  DispatchRuleDraft,
  ProviderFormState,
  StatusFormState,
  WorkspaceTab
} from "./types";

export {
  CHANNEL_SELECT_OPTIONS,
  CLAUDE_AGENT_SDK_PRELUDE_TEXT,
  CLAUDE_CODE_SYSTEM_PRELUDE_TEXT,
  LIVE_USAGE_CHANNELS,
  OAUTH_CHANNELS,
  OPERATION_OPTIONS,
  PROTOCOL_OPTIONS,
  buildChannelSettingsJson,
  buildCredentialSecretJson,
  credentialDefaultNameFromSecretJson,
  credentialDefaultNameFromSecretValues,
  buildBulkExportText,
  buildDispatchJson,
  createEmptyCredentialFormState,
  createDefaultDispatchRule,
  defaultBulkMode,
  credentialFormFromRow,
  credentialSchemaForChannel,
  secretValuesFromSecretJson,
  defaultChannelSettingsDraft,
  defaultDispatchRulesForChannel,
  isCustomChannel,
  normalizeChannel,
  normalizeDispatchRules,
  parseCredentialRoutingFlagsFromSettings,
  parseBulkCredentialText,
  parseChannelSettingsDraft,
  resolveProviderDispatchRules
};

export type {
  ChannelCredentialSchema,
  CredentialFieldSchema,
  CredentialFieldType,
  CredentialFieldValue,
  CredentialsSubTab,
  CredentialBulkMode,
  BulkCredentialImportEntry,
  CredentialFormState,
  DispatchMode,
  DispatchRuleDraft,
  ProviderFormState,
  StatusFormState,
  WorkspaceTab,
  UsageDisplayKind,
  UsageDisplayRow,
  UsageSampleRow,
  LiveUsageRow,
  UsageWindow,
  UsageWindowSpec
};

export function createEmptyProviderFormState(): ProviderFormState {
  const channel = "custom";
  return {
    id: "",
    name: "",
    channel,
    credentialRoundRobinEnabled: true,
    credentialCacheAffinityEnabled: true,
    settings: defaultChannelSettingsDraft(channel),
    dispatchRules: defaultDispatchRulesForChannel(channel),
    enabled: true
  };
}

export function toProviderFormState(row: ProviderQueryRow): ProviderFormState {
  const credentialRoutingFlags = parseCredentialRoutingFlagsFromSettings(
    row.settings_json
  );
  return {
    id: String(row.id),
    name: row.name,
    channel: row.channel,
    credentialRoundRobinEnabled: credentialRoutingFlags.roundRobinEnabled,
    credentialCacheAffinityEnabled: credentialRoutingFlags.cacheAffinityEnabled,
    settings: parseChannelSettingsDraft(row.channel, row.settings_json),
    dispatchRules: resolveProviderDispatchRules(row.channel, row.dispatch_json),
    enabled: row.enabled
  };
}

export function formatJson(value: unknown): string {
  return JSON.stringify(value ?? {}, null, 2);
}

export function usagePayloadToText(payload: unknown): string {
  if (typeof payload === "string") {
    return payload;
  }
  return JSON.stringify(payload ?? {}, null, 2);
}

export { buildUsageDisplayRows, buildUsageWindowSpecs, formatUsagePercent, parseLiveUsageRows };

export function mergeQueryString(
  rawQuery: string,
  extras: Record<string, string | null | undefined>
): string {
  const input = rawQuery.trim();
  const params = new URLSearchParams(input.startsWith("?") ? input.slice(1) : input);
  for (const [key, value] of Object.entries(extras)) {
    if (value == null) {
      params.delete(key);
      continue;
    }
    const trimmed = value.trim();
    if (trimmed) {
      params.set(key, trimmed);
    } else {
      params.delete(key);
    }
  }
  const query = params.toString();
  return query ? `?${query}` : "";
}

export function supportsOAuth(channel: string): boolean {
  return OAUTH_CHANNELS.has(normalizeChannel(channel));
}

export function supportsUpstreamUsage(channel: string): boolean {
  return LIVE_USAGE_CHANNELS.has(normalizeChannel(channel));
}

export { availableBulkModes };
export { getChannelConfig };

export function hasKnownChannel(channel: string): boolean {
  return getChannelConfig(channel) !== null;
}

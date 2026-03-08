import type { ProviderChannelCatalogRow } from "../../../lib/types";

import {
  buildChannelSelectOptions,
  channelSupportsOAuth,
  channelSupportsUpstreamUsage
} from "./channels/catalog";
import { CHANNEL_CONFIGS } from "./channels/registry";

export const OPERATION_OPTIONS = [
  "ModelList",
  "ModelGet",
  "CountToken",
  "Compact",
  "GenerateContent",
  "OpenAiResponseWebSocket",
  "GeminiLive",
  "StreamGenerateContent",
  "CreateImage",
  "StreamCreateImage",
  "CreateImageEdit",
  "StreamCreateImageEdit",
  "CreateVideo",
  "VideoGet",
  "VideoContentGet",
  "Embedding"
];

export const PROTOCOL_OPTIONS = [
  "OpenAi",
  "Claude",
  "Gemini",
  "OpenAiChatCompletion",
  "GeminiNDJson"
];

export const CLAUDE_CODE_SYSTEM_PRELUDE_TEXT =
  "You are Claude Code, Anthropic's official CLI for Claude.";
export const CLAUDE_AGENT_SDK_PRELUDE_TEXT =
  "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

export const CHANNEL_SELECT_OPTIONS = buildChannelSelectOptions();

export function getChannelSelectOptions(
  channelCatalog?: ProviderChannelCatalogRow[] | null
): Array<{ value: string; label: string }> {
  return buildChannelSelectOptions(channelCatalog);
}

export function supportsOAuthInCatalog(
  channel: string,
  channelCatalog?: ProviderChannelCatalogRow[] | null
): boolean {
  return channelSupportsOAuth(channel, channelCatalog);
}

export function supportsUpstreamUsageInCatalog(
  channel: string,
  channelCatalog?: ProviderChannelCatalogRow[] | null
): boolean {
  return channelSupportsUpstreamUsage(channel, channelCatalog);
}

export function isCustomChannel(channel: string): boolean {
  const normalized = channel.trim().toLowerCase();
  if (!normalized) {
    return false;
  }
  return !(normalized in CHANNEL_CONFIGS);
}

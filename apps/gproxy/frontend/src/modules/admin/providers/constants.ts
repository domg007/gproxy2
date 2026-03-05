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

const BUILTIN_CHANNELS = Object.keys(CHANNEL_CONFIGS).filter((channel) => channel !== "custom");

export const CHANNEL_SELECT_OPTIONS = [
  { value: "custom", label: "custom" },
  ...BUILTIN_CHANNELS.map((channel) => ({ value: channel, label: channel }))
];

export const OAUTH_CHANNELS = new Set(
  Object.values(CHANNEL_CONFIGS)
    .filter((config) => config.supportsOAuth)
    .map((config) => config.channel)
);

export const LIVE_USAGE_CHANNELS = new Set(
  Object.values(CHANNEL_CONFIGS)
    .filter((config) => config.supportsUpstreamUsage)
    .map((config) => config.channel)
);

export function isCustomChannel(channel: string): boolean {
  const normalized = channel.trim().toLowerCase();
  if (!normalized) {
    return false;
  }
  return !(normalized in CHANNEL_CONFIGS);
}

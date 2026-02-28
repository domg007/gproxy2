import type { ChannelCredentialSchema, ChannelSettingsDraft, TemplateRoute } from "../types";
import type { ChannelOAuthUi } from "./oauth";
import { CHANNEL_CONFIG as customConfig } from "./custom";
import { CHANNEL_CONFIG as openaiConfig } from "./openai";
import { CHANNEL_CONFIG as claudeConfig } from "./claude";
import { CHANNEL_CONFIG as aistudioConfig } from "./aistudio";
import { CHANNEL_CONFIG as vertexConfig } from "./vertex";
import { CHANNEL_CONFIG as vertexexpressConfig } from "./vertexexpress";
import { CHANNEL_CONFIG as geminicliConfig } from "./geminicli";
import { CHANNEL_CONFIG as antigravityConfig } from "./antigravity";
import { CHANNEL_CONFIG as claudecodeConfig } from "./claudecode";
import { CHANNEL_CONFIG as codexConfig } from "./codex";
import { CHANNEL_CONFIG as nvidiaConfig } from "./nvidia";
import { CHANNEL_CONFIG as deepseekConfig } from "./deepseek";

export type ChannelConfig = {
  channel: string;
  supportsOAuth: boolean;
  supportsUpstreamUsage: boolean;
  defaultSettingsDraft: () => ChannelSettingsDraft;
  parseSettingsDraft: (value: unknown) => ChannelSettingsDraft;
  buildSettingsJson: (settings: ChannelSettingsDraft) => Record<string, unknown>;
  oauthUi?: ChannelOAuthUi;
  credentialSchema: ChannelCredentialSchema;
  dispatchTemplateRoutes: readonly TemplateRoute[];
};

export const CHANNEL_CONFIGS: Record<string, ChannelConfig> = {
  [customConfig.channel]: customConfig,
  [openaiConfig.channel]: openaiConfig,
  [claudeConfig.channel]: claudeConfig,
  [aistudioConfig.channel]: aistudioConfig,
  [vertexConfig.channel]: vertexConfig,
  [vertexexpressConfig.channel]: vertexexpressConfig,
  [geminicliConfig.channel]: geminicliConfig,
  [antigravityConfig.channel]: antigravityConfig,
  [claudecodeConfig.channel]: claudecodeConfig,
  [codexConfig.channel]: codexConfig,
  [nvidiaConfig.channel]: nvidiaConfig,
  [deepseekConfig.channel]: deepseekConfig,
};

export function getChannelConfig(channel: string): ChannelConfig | null {
  const normalized = channel.trim().toLowerCase();
  if (!normalized) {
    return null;
  }
  return CHANNEL_CONFIGS[normalized] ?? null;
}

export const BUILTIN_CHANNELS = Object.keys(CHANNEL_CONFIGS);

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
import { CHANNEL_CONFIG as groqConfig } from "./groq";

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

type ChannelBaseConfig = Omit<ChannelConfig, "supportsOAuth" | "supportsUpstreamUsage">;

type ChannelRegistryEntry = {
  config: ChannelBaseConfig;
  supportsOAuth: boolean;
  supportsUpstreamUsage: boolean;
};

const CHANNEL_REGISTRY: readonly ChannelRegistryEntry[] = [
  { config: customConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: openaiConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: claudeConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: aistudioConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: vertexConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: vertexexpressConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: geminicliConfig, supportsOAuth: true, supportsUpstreamUsage: true },
  { config: antigravityConfig, supportsOAuth: true, supportsUpstreamUsage: true },
  { config: claudecodeConfig, supportsOAuth: true, supportsUpstreamUsage: true },
  { config: codexConfig, supportsOAuth: true, supportsUpstreamUsage: true },
  { config: nvidiaConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: deepseekConfig, supportsOAuth: false, supportsUpstreamUsage: false },
  { config: groqConfig, supportsOAuth: false, supportsUpstreamUsage: false },
];

export const CHANNEL_CONFIGS: Record<string, ChannelConfig> = Object.fromEntries(
  CHANNEL_REGISTRY.map(({ config, supportsOAuth, supportsUpstreamUsage }) => [
    config.channel,
    { ...config, supportsOAuth, supportsUpstreamUsage },
  ])
) as Record<string, ChannelConfig>;

export function getChannelConfig(channel: string): ChannelConfig | null {
  const normalized = channel.trim().toLowerCase();
  if (!normalized) {
    return null;
  }
  return CHANNEL_CONFIGS[normalized] ?? null;
}

export const BUILTIN_CHANNELS = CHANNEL_REGISTRY.map(({ config }) => config.channel);

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
  defaultSettingsDraft: () => ChannelSettingsDraft;
  parseSettingsDraft: (value: unknown) => ChannelSettingsDraft;
  buildSettingsJson: (settings: ChannelSettingsDraft) => Record<string, unknown>;
  oauthUi?: ChannelOAuthUi;
  credentialSchema: ChannelCredentialSchema;
  dispatchTemplateRoutes: readonly TemplateRoute[];
};

const CHANNEL_REGISTRY: readonly ChannelConfig[] = [
  customConfig,
  openaiConfig,
  claudeConfig,
  aistudioConfig,
  vertexConfig,
  vertexexpressConfig,
  geminicliConfig,
  antigravityConfig,
  claudecodeConfig,
  codexConfig,
  nvidiaConfig,
  deepseekConfig,
  groqConfig
];

export const CHANNEL_CONFIGS: Record<string, ChannelConfig> = Object.fromEntries(
  CHANNEL_REGISTRY.map((config) => [config.channel, config])
) as Record<string, ChannelConfig>;

export function getChannelConfig(channel: string): ChannelConfig | null {
  const normalized = channel.trim().toLowerCase();
  if (!normalized) {
    return null;
  }
  return CHANNEL_CONFIGS[normalized] ?? null;
}

export const BUILTIN_CHANNELS = CHANNEL_REGISTRY.map((config) => config.channel).filter(
  (channel) => channel !== "custom"
);

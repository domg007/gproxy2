import type { ProviderKind } from "./types";

export type FieldType = "text" | "password" | "number" | "textarea" | "boolean" | "select";

export type FieldOption = {
  value: string;
  labelKey?: string;
};

export type FieldSpec = {
  key: string;
  type: FieldType;
  required?: boolean;
  options?: FieldOption[];
};

export const CLAUDE_CODE_SYSTEM_PRELUDE = "claude_code_system";
export const CLAUDE_AGENT_SDK_PRELUDE = "claude_agent_sdk";

export const providerKinds: ProviderKind[] = [
  "openai",
  "claude",
  "aistudio",
  "vertexexpress",
  "vertex",
  "geminicli",
  "claudecode",
  "codex",
  "antigravity",
  "nvidia",
  "deepseek",
  "custom"
];

export function kindFromConfig(config: unknown): ProviderKind {
  if (!config || typeof config !== "object") {
    return "custom";
  }
  const raw = String((config as Record<string, unknown>).kind ?? "custom").toLowerCase();
  const hit = providerKinds.find((item) => item === raw);
  return hit ?? "custom";
}

export function channelSettingsFromConfig(config: unknown): Record<string, unknown> {
  if (!config || typeof config !== "object") {
    return {};
  }
  const raw = (config as Record<string, unknown>).channel_settings;
  if (raw && typeof raw === "object") {
    return raw as Record<string, unknown>;
  }
  return {};
}

export const configFieldMap: Record<ProviderKind, FieldSpec[]> = {
  openai: [{ key: "base_url", type: "text" }],
  claude: [{ key: "base_url", type: "text" }],
  aistudio: [{ key: "base_url", type: "text" }],
  vertexexpress: [{ key: "base_url", type: "text" }],
  vertex: [
    { key: "base_url", type: "text" },
    { key: "location", type: "text" },
    { key: "token_uri", type: "text" },
    { key: "oauth_token_url", type: "text" }
  ],
  geminicli: [{ key: "base_url", type: "text" }],
  claudecode: [
    { key: "base_url", type: "text" },
    { key: "claude_ai_base_url", type: "text" },
    { key: "platform_base_url", type: "text" },
    {
      key: "prelude_text",
      type: "select",
      options: [
        {
          value: CLAUDE_CODE_SYSTEM_PRELUDE,
          labelKey: "providers.prelude_option_claude_code_system"
        },
        {
          value: CLAUDE_AGENT_SDK_PRELUDE,
          labelKey: "providers.prelude_option_claude_agent_sdk"
        }
      ]
    }
  ],
  codex: [{ key: "base_url", type: "text" }],
  antigravity: [{ key: "base_url", type: "text" }],
  nvidia: [
    { key: "base_url", type: "text" },
    { key: "hf_token", type: "password" },
    { key: "hf_url", type: "text" },
    { key: "data_dir", type: "text" }
  ],
  deepseek: [{ key: "base_url", type: "text" }],
  custom: [
    { key: "id", type: "text", required: true },
    { key: "proto", type: "text", required: true },
    { key: "base_url", type: "text", required: true },
    { key: "count_tokens", type: "text" }
  ]
};

const configDefaultFieldMap: Partial<Record<ProviderKind, Record<string, string>>> = {
  openai: {
    base_url: "https://api.openai.com"
  },
  claude: {
    base_url: "https://api.anthropic.com"
  },
  aistudio: {
    base_url: "https://generativelanguage.googleapis.com"
  },
  vertexexpress: {
    base_url: "https://aiplatform.googleapis.com"
  },
  vertex: {
    base_url: "https://aiplatform.googleapis.com",
    location: "us-central1",
    token_uri: "https://oauth2.googleapis.com/token",
    oauth_token_url: "https://oauth2.googleapis.com/token"
  },
  geminicli: {
    base_url: "https://cloudcode-pa.googleapis.com"
  },
  claudecode: {
    base_url: "https://api.anthropic.com",
    claude_ai_base_url: "https://claude.ai",
    platform_base_url: "https://platform.claude.com",
    prelude_text: CLAUDE_CODE_SYSTEM_PRELUDE
  },
  codex: {
    base_url: "https://chatgpt.com/backend-api/codex"
  },
  antigravity: {
    base_url: "https://daily-cloudcode-pa.sandbox.googleapis.com"
  },
  nvidia: {
    base_url: "https://integrate.api.nvidia.com"
  },
  deepseek: {
    base_url: "https://api.deepseek.com"
  }
};

export function getConfigFieldDefault(kind: ProviderKind, key: string): string | undefined {
  return configDefaultFieldMap[kind]?.[key];
}

const apiKeyFields: FieldSpec[] = [{ key: "api_key", type: "password", required: true }];

export const credentialFieldMap: Record<ProviderKind, FieldSpec[]> = {
  openai: apiKeyFields,
  claude: apiKeyFields,
  aistudio: apiKeyFields,
  vertexexpress: apiKeyFields,
  nvidia: apiKeyFields,
  deepseek: apiKeyFields,
  custom: apiKeyFields,
  vertex: [
    { key: "project_id", type: "text", required: true },
    { key: "client_email", type: "text", required: true },
    { key: "private_key", type: "textarea", required: true },
    { key: "private_key_id", type: "text", required: true },
    { key: "client_id", type: "text", required: true },
    { key: "auth_uri", type: "text" },
    { key: "token_uri", type: "text" },
    { key: "auth_provider_x509_cert_url", type: "text" },
    { key: "client_x509_cert_url", type: "text" },
    { key: "universe_domain", type: "text" },
    { key: "access_token", type: "password", required: true },
    { key: "expires_at", type: "number", required: true }
  ],
  geminicli: [
    { key: "access_token", type: "password", required: true },
    { key: "refresh_token", type: "password", required: true },
    { key: "expires_at", type: "number", required: true },
    { key: "project_id", type: "text", required: true },
    { key: "client_id", type: "text", required: true },
    { key: "client_secret", type: "password", required: true },
    { key: "user_email", type: "text" }
  ],
  claudecode: [
    { key: "access_token", type: "password", required: true },
    { key: "refresh_token", type: "password", required: true },
    { key: "expires_at", type: "number", required: true },
    { key: "subscription_type", type: "text", required: true },
    { key: "rate_limit_tier", type: "text", required: true },
    { key: "session_key", type: "text" },
    { key: "enable_claude_1m_sonnet", type: "boolean" },
    { key: "enable_claude_1m_opus", type: "boolean" },
    { key: "supports_claude_1m_sonnet", type: "boolean" },
    { key: "supports_claude_1m_opus", type: "boolean" },
    { key: "user_email", type: "text" }
  ],
  codex: [
    { key: "access_token", type: "password", required: true },
    { key: "refresh_token", type: "password", required: true },
    { key: "id_token", type: "password", required: true },
    { key: "account_id", type: "text", required: true },
    { key: "expires_at", type: "number", required: true },
    { key: "user_email", type: "text" }
  ],
  antigravity: [
    { key: "access_token", type: "password", required: true },
    { key: "refresh_token", type: "password", required: true },
    { key: "expires_at", type: "number", required: true },
    { key: "project_id", type: "text", required: true },
    { key: "client_id", type: "text", required: true },
    { key: "client_secret", type: "password", required: true },
    { key: "user_email", type: "text" }
  ]
};

const credentialTagMap: Record<ProviderKind, string> = {
  openai: "OpenAI",
  claude: "Claude",
  aistudio: "AIStudio",
  vertexexpress: "VertexExpress",
  vertex: "Vertex",
  geminicli: "GeminiCli",
  claudecode: "ClaudeCode",
  codex: "Codex",
  antigravity: "Antigravity",
  nvidia: "Nvidia",
  deepseek: "DeepSeek",
  custom: "Custom"
};

export function buildProviderConfig(kind: ProviderKind, fields: Record<string, string>): Record<string, unknown> {
  const channelSettings: Record<string, unknown> = {};
  if (kind === "claudecode" && !fields.platform_base_url && fields.console_base_url) {
    channelSettings.platform_base_url = fields.console_base_url.trim();
  }
  for (const spec of configFieldMap[kind]) {
    const raw = fields[spec.key] ?? "";
    const text = raw.trim();
    if (!text) {
      continue;
    }
    if (spec.type === "number") {
      channelSettings[spec.key] = Number(text);
      continue;
    }
    if (spec.type === "boolean") {
      channelSettings[spec.key] = text === "true";
      continue;
    }
    channelSettings[spec.key] = text;
  }
  return {
    kind,
    channel_settings: channelSettings
  };
}

export function buildCredentialSecret(kind: ProviderKind, fields: Record<string, string>): Record<string, unknown> {
  const payload: Record<string, unknown> = {};
  for (const spec of credentialFieldMap[kind]) {
    const raw = fields[spec.key] ?? "";
    const text = raw.trim();
    if (!text) {
      continue;
    }
    if (spec.type === "number") {
      payload[spec.key] = Number(text);
      continue;
    }
    if (spec.type === "boolean") {
      const lower = text.toLowerCase();
      if (lower === "true" || lower === "false") {
        payload[spec.key] = lower === "true";
      }
      continue;
    }
    payload[spec.key] = text;
  }
  return {
    [credentialTagMap[kind]]: payload
  };
}

export function extractCredentialFields(kind: ProviderKind, secretJson: unknown): Record<string, string> {
  if (!secretJson || typeof secretJson !== "object") {
    return {};
  }
  const tag = credentialTagMap[kind];
  const payload = (secretJson as Record<string, unknown>)[tag];
  if (!payload || typeof payload !== "object") {
    return {};
  }
  const result: Record<string, string> = {};
  for (const [key, value] of Object.entries(payload as Record<string, unknown>)) {
    if (value === null || value === undefined) {
      continue;
    }
    result[key] = String(value);
  }
  return result;
}

export function extractConfigFields(kind: ProviderKind, configJson: unknown): Record<string, string> {
  const channelSettings = channelSettingsFromConfig(configJson);
  const result: Record<string, string> = {};
  if (kind === "claudecode") {
    const legacy = channelSettings.console_base_url;
    if (legacy !== null && legacy !== undefined && channelSettings.platform_base_url == null) {
      channelSettings.platform_base_url = legacy;
    }
    const legacyPrelude =
      channelSettings.prelude_txt ??
      channelSettings.prelude_text;
    if (legacyPrelude !== null && legacyPrelude !== undefined && channelSettings.prelude_text == null) {
      channelSettings.prelude_text = mapClaudeCodePrelude(String(legacyPrelude));
    } else if (channelSettings.prelude_text != null) {
      channelSettings.prelude_text = mapClaudeCodePrelude(String(channelSettings.prelude_text));
    }
  }
  for (const spec of configFieldMap[kind]) {
    const value = channelSettings[spec.key];
    if (value === null || value === undefined) {
      result[spec.key] = "";
      continue;
    }
    result[spec.key] = String(value);
  }
  return result;
}

function mapClaudeCodePrelude(value: string): string {
  const text = value.trim();
  if (!text) {
    return CLAUDE_CODE_SYSTEM_PRELUDE;
  }
  if (
    text === "You are a Claude agent, built on Anthropic's Claude Agent SDK." ||
    text.toLowerCase() === "claude_agent_sdk" ||
    text.toLowerCase() === "claude_agent" ||
    text.toLowerCase() === "agent_sdk"
  ) {
    return CLAUDE_AGENT_SDK_PRELUDE;
  }
  return CLAUDE_CODE_SYSTEM_PRELUDE;
}

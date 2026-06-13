export type SecretFamily = "api_key" | "oauth_tokens" | "service_account" | "github_token";
export type LoginMode = "authcode" | "device" | "cookie";

export interface ChannelMeta {
  id: string;
  family: SecretFamily;
  loginModes: LoginMode[];
  /** GET /admin/credentials/{id}/usage supported */
  usage: boolean;
  /** Prefill for the manual secret editor */
  secretTemplate: Record<string, unknown>;
  /** providers:secret.* extra hint key, if any */
  hintKey?: string;
}

const API_KEY_IDS = [
  "openai", "openrouter", "deepseek", "groq", "nvidia",
  "vercel", "custom", "claude_api", "aistudio", "vertexexpress",
] as const;

const OAUTH_TOKENS = { access_token: "", refresh_token: "" };

export const CHANNELS: ChannelMeta[] = [
  ...API_KEY_IDS.map((id) => ({
    id: id as string,
    family: "api_key" as const,
    loginModes: [] as LoginMode[],
    usage: false,
    secretTemplate: { api_key: "" },
  })),
  {
    id: "vertex",
    family: "service_account",
    loginModes: [],
    usage: false,
    secretTemplate: { client_email: "", private_key: "", project_id: "" },
  },
  {
    id: "geminicli",
    family: "oauth_tokens",
    loginModes: ["authcode"],
    usage: true,
    secretTemplate: { ...OAUTH_TOKENS, project_id: "" },
    hintKey: "geminiHint",
  },
  {
    id: "antigravity",
    family: "oauth_tokens",
    loginModes: ["authcode"],
    usage: true,
    secretTemplate: { ...OAUTH_TOKENS, project_id: "" },
    hintKey: "geminiHint",
  },
  {
    id: "claudecode",
    family: "oauth_tokens",
    loginModes: ["authcode", "cookie"],
    usage: true,
    secretTemplate: { ...OAUTH_TOKENS },
  },
  {
    id: "codex",
    family: "oauth_tokens",
    loginModes: ["authcode"],
    usage: true,
    secretTemplate: { ...OAUTH_TOKENS, account_id: "" },
  },
  {
    id: "kiro",
    family: "oauth_tokens",
    loginModes: ["authcode"],
    usage: true,
    secretTemplate: { ...OAUTH_TOKENS, auth_method: "social" },
  },
  {
    id: "copilot_cli",
    family: "github_token",
    loginModes: ["device"],
    usage: true,
    secretTemplate: { github_token: "" },
  },
];

export function channelMeta(id: string): ChannelMeta | undefined {
  return CHANNELS.find((c) => c.id === id);
}

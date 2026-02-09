export type AdminGlobalConfig = {
  host: string;
  port: number;
  admin_key: string;
  proxy?: string | null;
  dsn: string;
  event_redact_sensitive: boolean;
};

export type ProviderSummary = {
  id: number;
  name: string;
  enabled: boolean;
  updated_at: string;
};

export type ProviderDetail = {
  id: number;
  name: string;
  enabled: boolean;
  config_json: Record<string, unknown>;
  updated_at: string;
};

export type CredentialRow = {
  id: number;
  name?: string | null;
  settings_json: Record<string, unknown>;
  secret_json: Record<string, unknown>;
  enabled: boolean;
  created_at: string;
  updated_at: string;
  runtime_status?: CredentialRuntimeStatus;
};

export type CredentialListRow = {
  id: number;
  provider_id: number;
  name?: string | null;
  settings_json: Record<string, unknown>;
  enabled: boolean;
  created_at: string;
  updated_at: string;
  runtime_status?: CredentialRuntimeStatus;
};

export type CredentialUnavailableInfo = {
  reason: string;
  remaining_secs: number;
  remaining_ms?: number;
  until_epoch_ms?: number | null;
};

export type ModelUnavailableInfo = {
  model: string;
  reason: string;
  remaining_secs: number;
  remaining_ms?: number;
  until_epoch_ms?: number | null;
};

export type CredentialRuntimeStatus = {
  summary: "active" | "partial_unavailable" | "fully_unavailable" | "disabled";
  credential_unavailable: CredentialUnavailableInfo | null;
  model_unavailable: ModelUnavailableInfo[];
};

export type UserRow = {
  id: number;
  name: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
};

export type UserKeyRow = {
  id: number;
  user_id: number;
  label?: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
};

export type UsageResponse = {
  scope: string;
  provider?: string;
  credential_id?: number;
  model?: string;
  from: string;
  to: string;
  matched_rows: number;
  call_count: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_input_tokens: number;
  cache_creation_input_tokens: number;
  total_tokens: number;
};

export type ToastKind = "success" | "error" | "info";

export type ToastState = {
  kind: ToastKind;
  message: string;
} | null;

export type ProviderKind =
  | "openai"
  | "claude"
  | "aistudio"
  | "vertexexpress"
  | "vertex"
  | "geminicli"
  | "claudecode"
  | "codex"
  | "antigravity"
  | "nvidia"
  | "deepseek"
  | "custom";

export type OAuthStartResponse = {
  mode?: string;
  auth_url?: string;
  verification_uri?: string;
  user_code?: string;
  interval?: number;
  state?: string;
  redirect_uri?: string;
  instructions?: string;
  [key: string]: unknown;
};

export type OAuthCallbackResponse = {
  [key: string]: unknown;
};

export type SelfUpdateResponse = {
  ok: boolean;
  from_version: string;
  release_tag: string;
  asset: string;
  installed_to: string;
  restart_required: boolean;
  restart_scheduled?: boolean;
  note?: string;
};

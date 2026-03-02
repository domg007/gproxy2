export type Scope<T> = "All" | { Eq: T };

export type UserRole = "admin" | "user";
export type ThemeMode = "light" | "dark" | "system";

export interface GlobalSettingsRow {
  id: number;
  host: string;
  port: number;
  admin_key: string;
  hf_token: string | null;
  hf_url: string | null;
  proxy: string | null;
  spoof_emulation: string | null;
  dsn: string;
  data_dir: string;
  mask_sensitive_info: boolean;
  updated_at: string;
}

export interface ProviderQueryRow {
  id: number;
  name: string;
  channel: string;
  settings_json: Record<string, unknown>;
  dispatch_json: Record<string, unknown>;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface CredentialQueryRow {
  id: number;
  provider_id: number;
  name: string | null;
  kind: string;
  settings_json: Record<string, unknown> | null;
  secret_json: Record<string, unknown>;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface CredentialStatusQueryRow {
  id: number;
  credential_id: number;
  channel: string;
  health_kind: string;
  health_json: Record<string, unknown> | null;
  checked_at: string | null;
  last_error: string | null;
  updated_at: string;
}

export interface UserQueryRow {
  id: number;
  name: string;
  password: string;
  enabled: boolean;
}

export interface UserKeyQueryRow {
  id: number;
  user_id: number;
  api_key: string;
}

export interface UpstreamRequestQueryRow {
  trace_id: number;
  downstream_trace_id: number | null;
  at: string;
  internal: boolean;
  provider_id: number | null;
  credential_id: number | null;
  request_method: string;
  request_headers_json: Record<string, unknown>;
  request_url: string | null;
  request_body: number[] | null;
  response_status: number | null;
  response_headers_json: Record<string, unknown>;
  response_body: number[] | null;
  created_at: string;
}

export interface DownstreamRequestQueryRow {
  trace_id: number;
  at: string;
  internal: boolean;
  user_id: number | null;
  user_key_id: number | null;
  operation: string | null;
  protocol: string | null;
  request_method: string;
  request_headers_json: Record<string, unknown>;
  request_path: string;
  request_query: string | null;
  request_body: number[] | null;
  response_status: number | null;
  response_headers_json: Record<string, unknown>;
  response_body: number[] | null;
  created_at: string;
}

export interface RequestQueryCount {
  count: number;
}

export interface UsageQueryRow {
  trace_id: number;
  upstream_trace_id: number;
  downstream_trace_id: number | null;
  at: string;
  provider_id: number | null;
  provider_channel: string | null;
  credential_id: number | null;
  user_id: number | null;
  user_key_id: number | null;
  operation: string;
  protocol: string;
  model: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_read_input_tokens: number | null;
  cache_creation_input_tokens: number | null;
  cache_creation_input_tokens_5min: number | null;
  cache_creation_input_tokens_1h: number | null;
}

export interface UsageSummary {
  count: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_input_tokens: number;
  cache_creation_input_tokens: number;
  cache_creation_input_tokens_5min: number;
  cache_creation_input_tokens_1h: number;
}

export interface ApiErrorData {
  status: number;
  message: string;
}

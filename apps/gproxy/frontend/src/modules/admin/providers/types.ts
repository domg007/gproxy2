export type DispatchMode = "passthrough" | "transform" | "local" | "unsupported";
export type WorkspaceTab = "config" | "credentials";
export type CredentialsSubTab = "single" | "bulk" | "oauth";
export type CredentialBulkMode = "keys" | "json" | "claudecode_cookie";
export type CredentialPickMode =
  | "sticky_no_cache"
  | "sticky_with_cache"
  | "round_robin_with_cache"
  | "round_robin_no_cache";

export type DispatchRuleDraft = {
  id: string;
  srcOperation: string;
  srcProtocol: string;
  mode: DispatchMode;
  dstOperation: string;
  dstProtocol: string;
};

export type TemplateRoute = readonly [string, string, string, string, DispatchMode?];

export type ChannelSettingsDraft = Record<string, string>;

export type ProviderFormState = {
  id: string;
  name: string;
  channel: string;
  credentialPickMode: CredentialPickMode;
  settings: ChannelSettingsDraft;
  dispatchRules: DispatchRuleDraft[];
  enabled: boolean;
};

export type CredentialFieldType =
  | "string"
  | "integer"
  | "boolean"
  | "optional_string"
  | "optional_boolean";

export type CredentialFieldValue = string | boolean | null;

export type CredentialFieldSchema = {
  key: string;
  label: string;
  type: CredentialFieldType;
  placeholder?: string;
};

export type ChannelCredentialSchema = {
  channel: string;
  kind: string;
  wrapper: "Builtin" | "Custom";
  builtinVariant?: string;
  fields: CredentialFieldSchema[];
};

export type CredentialFormState = {
  id: string;
  name: string;
  kind: string;
  secretValues: Record<string, CredentialFieldValue>;
  settingsPayload: Record<string, unknown> | null;
  enabled: boolean;
};

export type BulkCredentialImportEntry = {
  id?: number;
  name?: string | null;
  enabled?: boolean;
  settingsPayload?: Record<string, unknown> | null;
  secretValues: Record<string, CredentialFieldValue>;
};

export type StatusFormState = {
  id: string;
  credentialId: string;
  healthKind: string;
  healthJson: string;
  checkedAtUnixMs: string;
  lastError: string;
};

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { request, formatApiError, safeParseJson } from "../lib/api";
import type {
  CredentialRow,
  OAuthCallbackResponse,
  OAuthStartResponse,
  ProviderDetail,
  ProviderSummary
} from "../lib/types";
import { formatDateTime } from "../lib/format";
import { formatUsagePercent, parseLiveUsageRows, type LiveUsageRow } from "../lib/live_usage";
import {
  buildImportedCredentialFromJson,
  buildImportedCredentialFromKey,
  buildJsonCredentialTemplate,
  isJsonCredentialKind,
  parseJsonCredentialText
} from "../lib/credential_import";
import {
  buildProviderConfig,
  channelSettingsFromConfig,
  configFieldMap,
  extractConfigFields,
  getConfigFieldDefault,
  kindFromConfig
} from "../lib/provider_schema";
import { Badge, Button, Card, FieldLabel, TextArea, TextInput } from "../components/ui";
import { useI18n } from "../i18n";

type Props = {
  adminKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
};

type WorkspaceTab = "config" | "credentials" | "oauth";
type CustomProto = "claude" | "gemini" | "openai" | "openai_chat" | "openai_response";
type CountTokensMode = "upstream" | "tokenizers" | "tiktoken";
type DispatchMode = "native" | "transform" | "unsupported";
type DispatchRowDraft = {
  opIndex: number;
  opName: string;
  mode: DispatchMode;
  target: CustomProto;
};
type CustomModelDraft = {
  id: string;
  displayName: string;
};
type CustomConfigDraft = {
  id: string;
  enabled: boolean;
  proto: CustomProto;
  baseUrl: string;
  countTokens: CountTokensMode;
  jsonParamMaskText: string;
  dispatchRows: DispatchRowDraft[];
  useModelTable: boolean;
  models: CustomModelDraft[];
};
type OAuthUiDefaults = {
  redirectUri?: string;
  scope?: string;
};
type ClaudeCodeOneMField =
  | "enable_claude_1m_sonnet"
  | "enable_claude_1m_opus"
  | "supports_claude_1m_sonnet"
  | "supports_claude_1m_opus";
type ClaudeCodeOneMFlags = {
  enable_claude_1m_sonnet?: boolean;
  enable_claude_1m_opus?: boolean;
  supports_claude_1m_sonnet?: boolean;
  supports_claude_1m_opus?: boolean;
};

function asJsonObject(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function unwrapCredentialSecretForDisplay(secret: Record<string, unknown>): {
  wrapperKey: string | null;
  payload: Record<string, unknown>;
} {
  const entries = Object.entries(secret);
  if (entries.length === 1) {
    const [key, value] = entries[0];
    const inner = asJsonObject(value);
    if (inner) {
      return {
        wrapperKey: key,
        payload: inner
      };
    }
  }
  return {
    wrapperKey: null,
    payload: secret
  };
}

function asBoolean(value: unknown): boolean | undefined {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (normalized === "true") {
      return true;
    }
    if (normalized === "false") {
      return false;
    }
  }
  return undefined;
}

function extractClaudeCodeOneMFlags(secret: Record<string, unknown>): ClaudeCodeOneMFlags {
  const { payload } = unwrapCredentialSecretForDisplay(secret);
  return {
    enable_claude_1m_sonnet: asBoolean(payload.enable_claude_1m_sonnet),
    enable_claude_1m_opus: asBoolean(payload.enable_claude_1m_opus),
    supports_claude_1m_sonnet: asBoolean(payload.supports_claude_1m_sonnet),
    supports_claude_1m_opus: asBoolean(payload.supports_claude_1m_opus)
  };
}

function patchClaudeCodeOneMFlag(
  secret: Record<string, unknown>,
  key: ClaudeCodeOneMField,
  value: boolean
): Record<string, unknown> {
  const display = unwrapCredentialSecretForDisplay(secret);
  const nextPayload: Record<string, unknown> = {
    ...display.payload,
    [key]: value
  };
  if (display.wrapperKey) {
    return {
      ...secret,
      [display.wrapperKey]: nextPayload
    };
  }
  return nextPayload;
}

function runtimeSummaryClass(summary: string): string {
  switch (summary) {
    case "fully_unavailable":
      return "border border-rose-200 bg-rose-50 text-rose-700";
    case "partial_unavailable":
      return "border border-amber-200 bg-amber-50 text-amber-700";
    case "disabled":
      return "border border-slate-200 bg-slate-100 text-slate-600";
    default:
      return "border border-emerald-200 bg-emerald-50 text-emerald-700";
  }
}

const OAUTH_PROVIDERS = new Set(["codex", "claudecode", "geminicli", "antigravity"]);
const LIVE_USAGE_PROVIDERS = new Set(["codex", "claudecode", "geminicli", "antigravity"]);
const CUSTOM_PROTO_OPTIONS: CustomProto[] = [
  "claude",
  "gemini",
  "openai_chat",
  "openai_response",
  "openai"
];
const CUSTOM_DISPATCH_OPERATION_NAMES = [
  "claude_generate",
  "claude_generate_stream",
  "claude_count_tokens",
  "claude_models_list",
  "claude_models_get",
  "gemini_generate",
  "gemini_generate_stream",
  "gemini_count_tokens",
  "gemini_models_list",
  "gemini_models_get",
  "openai_chat_generate",
  "openai_chat_generate_stream",
  "openai_response_generate",
  "openai_response_generate_stream",
  "openai_input_tokens",
  "openai_models_list",
  "openai_models_get",
  "oauth_start",
  "oauth_callback",
  "usage"
] as const;

function opNativeProto(opIndex: number): CustomProto | null {
  if (opIndex >= 0 && opIndex <= 4) {
    return "claude";
  }
  if (opIndex >= 5 && opIndex <= 9) {
    return "gemini";
  }
  if (opIndex >= 10 && opIndex <= 11) {
    return "openai_chat";
  }
  if (opIndex >= 12 && opIndex <= 13) {
    return "openai_response";
  }
  if (opIndex >= 14 && opIndex <= 16) {
    return "openai";
  }
  return null;
}

function parseCustomProto(value: unknown): CustomProto {
  if (typeof value !== "string") {
    return "openai_response";
  }
  return CUSTOM_PROTO_OPTIONS.includes(value as CustomProto)
    ? (value as CustomProto)
    : "openai_response";
}

function parseCountTokensMode(value: unknown): CountTokensMode {
  if (value === "upstream" || value === "tokenizers" || value === "tiktoken") {
    return value;
  }
  return "upstream";
}

function parseJsonParamMaskText(value: unknown): string {
  if (!Array.isArray(value)) {
    return "";
  }
  return value
    .map((item) => String(item ?? "").trim())
    .filter((item) => item.length > 0)
    .join("\n");
}

function parseJsonParamMaskList(text: string): string[] {
  const values = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  return Array.from(new Set(values));
}

function oauthUiDefaults(providerName: string): OAuthUiDefaults {
  switch (providerName) {
    case "claudecode":
      return {
        redirectUri: "https://platform.claude.com/oauth/code/callback",
        scope: "user:profile user:inference user:sessions:claude_code"
      };
    case "geminicli":
      return {
        redirectUri: "https://codeassist.google.com/authcode",
        scope: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile"
      };
    case "antigravity":
      return {
        redirectUri: "http://localhost:51121/oauth-callback",
        scope: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs"
      };
    case "codex":
      return {
        redirectUri: "http://localhost:1455/auth/callback",
        scope: "openid profile email offline_access"
      };
    default:
      return {};
  }
}

function scalarEntries(source: Record<string, unknown>): Array<[string, string]> {
  return Object.entries(source)
    .filter(([, value]) => ["string", "number", "boolean"].includes(typeof value))
    .map(([key, value]) => [key, String(value)]);
}

function buildDefaultDispatchRows(proto: CustomProto): DispatchRowDraft[] {
  return CUSTOM_DISPATCH_OPERATION_NAMES.map((opName, opIndex) => {
    const nativeProto = opNativeProto(opIndex);
    if (!nativeProto) {
      return { opIndex, opName, mode: "unsupported", target: proto };
    }
    if (nativeProto === proto) {
      return { opIndex, opName, mode: "native", target: proto };
    }
    return { opIndex, opName, mode: "transform", target: proto };
  });
}

function parseDispatchRows(rawDispatch: unknown, fallbackProto: CustomProto): DispatchRowDraft[] {
  const defaults = buildDefaultDispatchRows(fallbackProto);
  const ops = (() => {
    if (!rawDispatch || typeof rawDispatch !== "object") {
      return null;
    }
    const rawOps = (rawDispatch as Record<string, unknown>).ops;
    return Array.isArray(rawOps) ? rawOps : null;
  })();

  if (!ops) {
    return defaults;
  }

  return defaults.map((row, index) => {
    const raw = ops[index];
    if (raw === "native") {
      return { ...row, mode: "native" };
    }
    if (raw === "unsupported") {
      return { ...row, mode: "unsupported" };
    }
    if (raw && typeof raw === "object") {
      const transform = (raw as Record<string, unknown>).transform;
      if (transform && typeof transform === "object") {
        const target = parseCustomProto((transform as Record<string, unknown>).target);
        return { ...row, mode: "transform", target };
      }
    }
    return row;
  });
}

function extractCustomDraft(configJson: unknown, providerEnabled: boolean): CustomConfigDraft {
  const settings = channelSettingsFromConfig(configJson);
  const proto = parseCustomProto(settings.proto);
  const modelTableRaw = settings.model_table;
  const modelRows: CustomModelDraft[] =
    modelTableRaw && typeof modelTableRaw === "object" && Array.isArray((modelTableRaw as Record<string, unknown>).models)
      ? ((modelTableRaw as Record<string, unknown>).models as unknown[])
          .map((item) => {
            if (!item || typeof item !== "object") {
              return null;
            }
            const record = item as Record<string, unknown>;
            return {
              id: String(record.id ?? "").trim(),
              displayName: String(record.display_name ?? "").trim()
            };
          })
          .filter((item): item is CustomModelDraft => Boolean(item && item.id))
      : [];

  return {
    id: String(settings.id ?? ""),
    enabled: typeof settings.enabled === "boolean" ? settings.enabled : providerEnabled,
    proto,
    baseUrl: String(settings.base_url ?? ""),
    countTokens: parseCountTokensMode(settings.count_tokens),
    jsonParamMaskText: parseJsonParamMaskText(settings.json_param_mask),
    dispatchRows: parseDispatchRows(settings.dispatch, proto),
    useModelTable: modelTableRaw !== undefined,
    models: modelRows
  };
}

function toDispatchJson(rows: DispatchRowDraft[]): Record<string, unknown> {
  return {
    ops: rows.map((row) => {
      if (row.mode === "native") {
        return "native";
      }
      if (row.mode === "unsupported") {
        return "unsupported";
      }
      return { transform: { target: row.target } };
    })
  };
}

function buildCustomConfigJson(draft: CustomConfigDraft): Record<string, unknown> {
  const jsonParamMask = parseJsonParamMaskList(draft.jsonParamMaskText);
  const models = draft.models
    .map((row) => ({
      id: row.id.trim(),
      display_name: row.displayName.trim() || undefined
    }))
    .filter((row) => row.id);
  const channelSettings: Record<string, unknown> = {
    id: draft.id.trim(),
    enabled: draft.enabled,
    proto: draft.proto,
    base_url: draft.baseUrl.trim(),
    dispatch: toDispatchJson(draft.dispatchRows),
    count_tokens: draft.countTokens
  };
  if (jsonParamMask.length > 0) {
    channelSettings.json_param_mask = jsonParamMask;
  }
  if (draft.useModelTable) {
    channelSettings.model_table = { models };
  }
  return {
    kind: "custom",
    channel_settings: channelSettings
  };
}

export function ProvidersSection({ adminKey, notify }: Props) {
  const { t } = useI18n();
  const [items, setItems] = useState<ProviderDetail[]>([]);
  const [selectedName, setSelectedName] = useState<string>("");
  const [drafts, setDrafts] = useState<Record<string, Record<string, string>>>({});
  const [loading, setLoading] = useState(false);
  const [workspaceTab, setWorkspaceTab] = useState<WorkspaceTab>("config");
  const [creatingCustom, setCreatingCustom] = useState(false);
  const [customName, setCustomName] = useState("");
  const [customId, setCustomId] = useState("");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customProto, setCustomProto] = useState<CustomProto>("openai_response");
  const [customCountTokens, setCustomCountTokens] = useState<CountTokensMode>("upstream");
  const [customJsonParamMaskText, setCustomJsonParamMaskText] = useState("");
  const [customDispatchRows, setCustomDispatchRows] = useState<DispatchRowDraft[]>(
    buildDefaultDispatchRows("openai_response")
  );
  const [customUseModelTable, setCustomUseModelTable] = useState(false);
  const [customModels, setCustomModels] = useState<CustomModelDraft[]>([]);
  const [customCreating, setCustomCreating] = useState(false);
  const [customDeleting, setCustomDeleting] = useState(false);
  const [providerTogglingName, setProviderTogglingName] = useState<string | null>(null);
  const [customConfigDrafts, setCustomConfigDrafts] = useState<
    Record<string, CustomConfigDraft>
  >({});

  const [credentials, setCredentials] = useState<CredentialRow[]>([]);
  const [credentialsLoading, setCredentialsLoading] = useState(false);
  const [credentialImportText, setCredentialImportText] = useState("");
  const [credentialImportFiles, setCredentialImportFiles] = useState<File[]>([]);
  const [credentialImporting, setCredentialImporting] = useState(false);
  const credentialFileInputRef = useRef<HTMLInputElement | null>(null);
  const [credentialViewOpenIds, setCredentialViewOpenIds] = useState<Record<number, boolean>>({});
  const [credentialRuntimeOpenIds, setCredentialRuntimeOpenIds] = useState<Record<number, boolean>>(
    {}
  );
  const [credentialEditingId, setCredentialEditingId] = useState<number | null>(null);
  const [credentialEditName, setCredentialEditName] = useState("");
  const [credentialEditSecretJson, setCredentialEditSecretJson] = useState("");
  const [credentialSavingId, setCredentialSavingId] = useState<number | null>(null);
  const [credentialOneMSavingId, setCredentialOneMSavingId] = useState<number | null>(null);
  const [credentialCopiedId, setCredentialCopiedId] = useState<number | null>(null);
  const [quotaLoadingId, setQuotaLoadingId] = useState<number | null>(null);
  const [quotaRowsByCredentialId, setQuotaRowsByCredentialId] = useState<
    Record<number, LiveUsageRow[]>
  >({});

  const [oauthStartParams, setOauthStartParams] = useState({
    redirect_uri: "",
    scope: "",
    project_id: ""
  });
  const [oauthCallbackParams, setOauthCallbackParams] = useState({
    state: "",
    code: "",
    callback_url: "",
    project_id: ""
  });
  const [oauthStartResult, setOauthStartResult] = useState<OAuthStartResponse | null>(null);
  const [oauthCallbackResult, setOauthCallbackResult] = useState<OAuthCallbackResponse | null>(null);

  const selected = useMemo(
    () => items.find((item) => item.name === selectedName) ?? null,
    [items, selectedName]
  );

  const providerKind = selected ? kindFromConfig(selected.config_json) : null;
  const jsonCredentialKind = providerKind ? isJsonCredentialKind(providerKind) : false;

  const supportsOAuth = selected ? OAUTH_PROVIDERS.has(selected.name) : false;
  const supportsLiveUsage = selected ? LIVE_USAGE_PROVIDERS.has(selected.name) : false;
  const selectedIsCustom = providerKind === "custom";
  const selectedCustomKnownModels = useMemo(() => {
    if (!selectedIsCustom || !selected) {
      return [] as string[];
    }
    const customDraft =
      customConfigDrafts[selected.name] ?? extractCustomDraft(selected.config_json, selected.enabled);
    if (!customDraft.useModelTable) {
      return [] as string[];
    }
    return customDraft.models
      .map((row) => row.id.trim())
      .filter((id) => id.length > 0);
  }, [customConfigDrafts, selected, selectedIsCustom]);

  const tabs = useMemo(() => {
    const base: Array<{ id: WorkspaceTab; label: string }> = [
      { id: "config", label: t("providers.workspace_config") },
      { id: "credentials", label: t("nav.credentials") }
    ];
    if (supportsOAuth) {
      base.push({ id: "oauth", label: t("nav.oauth") });
    }
    return base;
  }, [supportsOAuth, t]);

  const loadProviders = useCallback(async () => {
    setLoading(true);
    try {
      const list = await request<{ providers: ProviderSummary[] }>("/admin/providers", {
        adminKey
      });
      const summaries = list.providers ?? [];
      const details = await Promise.all(
        summaries.map((provider) =>
          request<ProviderDetail>(`/admin/providers/${provider.name}`, { adminKey })
        )
      );
      setItems(details);

      const nextDrafts: Record<string, Record<string, string>> = {};
      const nextCustomDrafts: Record<string, CustomConfigDraft> = {};
      for (const provider of details) {
        const kind = kindFromConfig(provider.config_json);
        nextDrafts[provider.name] = extractConfigFields(kind, provider.config_json);
        if (kind === "custom") {
          nextCustomDrafts[provider.name] = extractCustomDraft(
            provider.config_json,
            provider.enabled
          );
        }
      }
      setDrafts(nextDrafts);
      setCustomConfigDrafts(nextCustomDrafts);

      if (!selectedName && details.length > 0) {
        setSelectedName(details[0].name);
      }
      if (selectedName && !details.find((item) => item.name === selectedName) && details.length > 0) {
        setSelectedName(details[0].name);
      }
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setLoading(false);
    }
  }, [adminKey, notify, selectedName]);

  const loadCredentials = useCallback(
    async (providerName: string) => {
      if (!providerName) {
        return;
      }
      setCredentialsLoading(true);
      try {
        const data = await request<{ credentials: CredentialRow[] }>(
          `/admin/providers/${providerName}/credentials`,
          { adminKey }
        );
        const rows = data.credentials ?? [];
        setCredentials(rows);
      } catch (error) {
        notify("error", formatApiError(error));
      } finally {
        setCredentialsLoading(false);
      }
    },
    [adminKey, notify]
  );

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    if (!selected) {
      return;
    }
    void loadCredentials(selected.name);
    setWorkspaceTab("config");
    setCredentialImportText("");
    setCredentialImportFiles([]);
    setCredentialImporting(false);
    setCredentialViewOpenIds({});
    setCredentialEditingId(null);
    setCredentialEditName("");
    setCredentialEditSecretJson("");
    setCredentialSavingId(null);
    setCredentialOneMSavingId(null);
    setCredentialCopiedId(null);
    setOauthStartResult(null);
    setOauthCallbackResult(null);
    setOauthStartParams({ redirect_uri: "", scope: "", project_id: "" });
    setOauthCallbackParams({ state: "", code: "", callback_url: "", project_id: "" });
    setQuotaRowsByCredentialId({});
    setQuotaLoadingId(null);
  }, [selected?.name]);

  useEffect(() => {
    if (!providerKind) {
      return;
    }
    if (isJsonCredentialKind(providerKind)) {
      setCredentialImportText(buildJsonCredentialTemplate(providerKind));
      return;
    }
    setCredentialImportText("");
  }, [providerKind]);

  useEffect(() => {
    if (!tabs.find((item) => item.id === workspaceTab)) {
      setWorkspaceTab("config");
    }
  }, [tabs, workspaceTab]);

  const updateCustomDraftByName = (
    providerName: string,
    updater: (draft: CustomConfigDraft) => CustomConfigDraft
  ) => {
    setCustomConfigDrafts((prev) => {
      const current =
        prev[providerName] ??
        extractCustomDraft(
          items.find((item) => item.name === providerName)?.config_json ?? {},
          items.find((item) => item.name === providerName)?.enabled ?? true
        );
      return {
        ...prev,
        [providerName]: updater(current)
      };
    });
  };

  const updateSelectedCustomDraft = (updater: (draft: CustomConfigDraft) => CustomConfigDraft) => {
    if (!selected) {
      return;
    }
    updateCustomDraftByName(selected.name, updater);
  };

  const saveProvider = async () => {
    if (!selected) {
      return;
    }
    try {
      const kind = kindFromConfig(selected.config_json);
      const configJson =
        kind === "custom"
          ? buildCustomConfigJson(
              customConfigDrafts[selected.name] ??
                extractCustomDraft(selected.config_json, selected.enabled)
            )
          : buildProviderConfig(kind, drafts[selected.name] ?? {});
      await request(`/admin/providers/${selected.name}`, {
        method: "PUT",
        adminKey,
        body: {
          enabled: selected.enabled,
          config_json: configJson
        }
      });
      notify("success", t("providers.saved"));
      await loadProviders();
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const toggleProviderEnabled = async (provider: ProviderDetail) => {
    setProviderTogglingName(provider.name);
    try {
      await request(`/admin/providers/${provider.name}`, {
        method: "PUT",
        adminKey,
        body: {
          enabled: !provider.enabled,
          config_json: provider.config_json
        }
      });
      notify("success", t("providers.toggle_ok"));
      await loadProviders();
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setProviderTogglingName(null);
    }
  };

  const deleteCredential = async (id: number) => {
    if (!selected) {
      return;
    }
    if (!confirm(`${t("common.delete")}? #${id}`)) {
      return;
    }
    try {
      await request(`/admin/credentials/${id}`, { method: "DELETE", adminKey });
      notify("success", t("credentials.delete_ok"));
      await loadCredentials(selected.name);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const toggleCredential = async (row: CredentialRow) => {
    if (!selected) {
      return;
    }
    try {
      await request(`/admin/credentials/${row.id}/enabled`, {
        method: "PUT",
        adminKey,
        body: { enabled: !row.enabled }
      });
      notify("success", t("credentials.toggle_ok"));
      await loadCredentials(selected.name);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const toggleCredentialView = (credentialId: number) => {
    setCredentialViewOpenIds((prev) => ({ ...prev, [credentialId]: !prev[credentialId] }));
  };

  const toggleCredentialRuntimeView = (credentialId: number) => {
    setCredentialRuntimeOpenIds((prev) => ({ ...prev, [credentialId]: !prev[credentialId] }));
  };

  const startCredentialEdit = (row: CredentialRow) => {
    setCredentialEditingId(row.id);
    setCredentialEditName(row.name ?? "");
    setCredentialEditSecretJson(JSON.stringify(row.secret_json, null, 2));
  };

  const cancelCredentialEdit = () => {
    setCredentialEditingId(null);
    setCredentialEditName("");
    setCredentialEditSecretJson("");
    setCredentialSavingId(null);
  };

  const saveCredentialEdit = async () => {
    if (!selected || credentialEditingId === null) {
      return;
    }
    const parsed = safeParseJson(credentialEditSecretJson.trim());
    const secretJson = asJsonObject(parsed);
    if (!secretJson) {
      notify("error", t("errors.invalid_json"));
      return;
    }

    setCredentialSavingId(credentialEditingId);
    try {
      await request(`/admin/credentials/${credentialEditingId}`, {
        method: "PUT",
        adminKey,
        body: {
          name: credentialEditName.trim() || null,
          secret_json: secretJson
        }
      });
      notify("success", t("credentials.update_ok"));
      await loadCredentials(selected.name);
      setCredentialEditingId(null);
      setCredentialEditName("");
      setCredentialEditSecretJson("");
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setCredentialSavingId(null);
    }
  };

  const oneMSupportLabel = (value: boolean | undefined) => {
    if (value === true) {
      return t("credentials.one_m_support_yes");
    }
    if (value === false) {
      return t("credentials.one_m_support_no");
    }
    return t("credentials.one_m_support_unknown");
  };

  const toggleClaudeCodeOneM = async (
    row: CredentialRow,
    key: ClaudeCodeOneMField,
    nextValue: boolean
  ) => {
    if (!selected) {
      return;
    }
    setCredentialOneMSavingId(row.id);
    try {
      const nextSecret = patchClaudeCodeOneMFlag(row.secret_json, key, nextValue);
      await request(`/admin/credentials/${row.id}`, {
        method: "PUT",
        adminKey,
        body: {
          name: row.name ?? null,
          secret_json: nextSecret
        }
      });
      notify("success", t("credentials.update_ok"));
      await loadCredentials(selected.name);
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setCredentialOneMSavingId(null);
    }
  };

  const copyCredentialSecret = async (credentialId: number, content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      setCredentialCopiedId(credentialId);
      notify("success", t("common.copied"));
      setTimeout(() => {
        setCredentialCopiedId((current) => (current === credentialId ? null : current));
      }, 1200);
    } catch {
      notify("error", t("errors.request_failed"));
    }
  };

  const runOAuthStart = async (mode?: string) => {
    if (!selected) {
      return;
    }
    try {
      const data = await request<OAuthStartResponse>(`/${selected.name}/oauth`, {
        userKey: adminKey,
        query: {
          mode: mode?.trim() || undefined,
          redirect_uri: oauthStartParams.redirect_uri.trim() || undefined,
          scope: oauthStartParams.scope.trim() || undefined,
          project_id: oauthStartParams.project_id.trim() || undefined
        }
      });
      setOauthStartResult(data);
      if (typeof data.state === "string") {
        setOauthCallbackParams((prev) => ({ ...prev, state: data.state as string }));
      }
      notify("success", t("oauth.start_ok"));
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const runtimeReasonText = (reason: string) => {
    const key = `credentials.reason_${reason}`;
    const translated = t(key);
    return translated === key ? reason : translated;
  };

  const runOAuthCallback = async () => {
    if (!selected) {
      return;
    }
    try {
      const data = await request<OAuthCallbackResponse>(`/${selected.name}/oauth/callback`, {
        userKey: adminKey,
        query: {
          state: oauthCallbackParams.state.trim() || undefined,
          code: oauthCallbackParams.code.trim() || undefined,
          callback_url: oauthCallbackParams.callback_url.trim() || undefined,
          project_id: oauthCallbackParams.project_id.trim() || undefined
        }
      });
      setOauthCallbackResult(data);
      notify("success", t("oauth.callback_ok"));
      await loadCredentials(selected.name);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const loadCredentialQuota = async (credentialId: number) => {
    if (!selected) {
      return;
    }
    if (!supportsLiveUsage) {
      notify("error", t("usage.live_unsupported"));
      return;
    }
    setQuotaLoadingId(credentialId);
    try {
      const data = await request<Record<string, unknown>>(`/${selected.name}/usage`, {
        userKey: adminKey,
        query: {
          credential_id: credentialId
        }
      });
      const rows = parseLiveUsageRows(selected.name, data);
      setQuotaRowsByCredentialId((prev) => ({ ...prev, [credentialId]: rows }));
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setQuotaLoadingId(null);
    }
  };

  const runCredentialImport = async () => {
    if (!selected || !providerKind) {
      notify("error", t("errors.missing_provider"));
      return;
    }
    setCredentialImporting(true);
    try {
      const payloads: Array<{ name: string | null; secretJson: Record<string, unknown> }> = [];
      if (jsonCredentialKind) {
        if (credentialImportText.trim()) {
          const parsed = parseJsonCredentialText(credentialImportText);
          for (const item of parsed) {
            const payload = buildImportedCredentialFromJson(providerKind, item);
            if (payload) {
              payloads.push(payload);
            }
          }
        }
        for (const file of credentialImportFiles) {
          const parsed = parseJsonCredentialText(await file.text());
          for (const item of parsed) {
            const payload = buildImportedCredentialFromJson(providerKind, item);
            if (payload) {
              payloads.push(payload);
            }
          }
        }
      } else if (credentialImportText.trim()) {
        const keys = credentialImportText
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter(Boolean);
        for (const key of keys) {
          payloads.push(buildImportedCredentialFromKey(providerKind, key));
        }
      }

      if (payloads.length === 0) {
        notify("info", t("common.empty"));
        return;
      }

      for (const payload of payloads) {
        await request(`/admin/providers/${selected.name}/credentials`, {
          method: "POST",
          adminKey,
          body: {
            name: payload.name,
            settings_json: {},
            secret_json: payload.secretJson,
            enabled: true
          }
        });
      }

      notify("success", `${t("credentials.import_ok")} (${payloads.length})`);
      setCredentialImportFiles([]);
      if (jsonCredentialKind) {
        setCredentialImportText(buildJsonCredentialTemplate(providerKind));
      } else {
        setCredentialImportText("");
      }
      await loadCredentials(selected.name);
    } catch (error) {
      if (error instanceof Error && error.message.includes("Invalid JSON document")) {
        notify("error", t("errors.invalid_json"));
      } else {
        notify("error", formatApiError(error));
      }
    } finally {
      setCredentialImporting(false);
    }
  };

  const createCustomProvider = async () => {
    const name = customName.trim();
    const baseUrl = customBaseUrl.trim();
    if (!name || !baseUrl) {
      notify("error", t("errors.missing_provider"));
      return;
    }
    if (!/^[A-Za-z0-9._-]+$/.test(name)) {
      notify("error", t("errors.invalid_provider_name"));
      return;
    }

    setCustomCreating(true);
    try {
      const id = customId.trim() || `custom-${name}`;
      const draft: CustomConfigDraft = {
        id,
        enabled: true,
        proto: customProto,
        baseUrl,
        countTokens: customCountTokens,
        jsonParamMaskText: customJsonParamMaskText,
        dispatchRows: customDispatchRows,
        useModelTable: customUseModelTable,
        models: customModels
      };
      const configJson = buildCustomConfigJson(draft);

      await request(`/admin/providers/${name}`, {
        method: "PUT",
        adminKey,
        body: {
          enabled: true,
          config_json: configJson
        }
      });

      setCreatingCustom(false);
      setCustomName("");
      setCustomId("");
      setCustomBaseUrl("");
      setCustomProto("openai_response");
      setCustomCountTokens("upstream");
      setCustomJsonParamMaskText("");
      setCustomDispatchRows(buildDefaultDispatchRows("openai_response"));
      setCustomUseModelTable(false);
      setCustomModels([]);

      await loadProviders();
      setSelectedName(name);
      notify("success", t("providers.custom_created"));
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setCustomCreating(false);
    }
  };

  const deleteCustomProvider = async () => {
    if (!selected || !selectedIsCustom) {
      return;
    }
    if (!confirm(t("providers.custom_delete_confirm", { provider: selected.name }))) {
      return;
    }
    setCustomDeleting(true);
    try {
      await request(`/admin/providers/${selected.name}`, {
        method: "DELETE",
        adminKey
      });
      notify("success", t("providers.custom_deleted"));
      await loadProviders();
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setCustomDeleting(false);
    }
  };

  const renderDispatchEditor = (
    rows: DispatchRowDraft[],
    onModeChange: (opIndex: number, mode: DispatchMode) => void,
    onTargetChange: (opIndex: number, target: CustomProto) => void,
    onReset: () => void
  ) => (
    <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-sm font-semibold text-slate-900">{t("providers.dispatch_title")}</div>
          <div className="mt-1 text-xs text-slate-500">{t("providers.dispatch_hint")}</div>
        </div>
        <Button variant="neutral" onClick={onReset}>
          {t("providers.dispatch_reset")}
        </Button>
      </div>
      <div className="space-y-2">
        {rows.map((row) => (
          <div
            key={row.opIndex}
            className="rounded-xl border border-slate-200 bg-white px-3 py-3 text-sm"
          >
            <div className="font-mono text-xs text-slate-700">{row.opName}</div>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <button
                type="button"
                className={`btn ${row.mode === "native" ? "btn-primary" : "btn-neutral"}`}
                onClick={() => onModeChange(row.opIndex, "native")}
              >
                {t("providers.dispatch_native")}
              </button>
              <button
                type="button"
                className={`btn ${row.mode === "transform" ? "btn-primary" : "btn-neutral"}`}
                onClick={() => onModeChange(row.opIndex, "transform")}
              >
                {t("providers.dispatch_transform")}
              </button>
              <button
                type="button"
                className={`btn ${row.mode === "unsupported" ? "btn-primary" : "btn-neutral"}`}
                onClick={() => onModeChange(row.opIndex, "unsupported")}
              >
                {t("providers.dispatch_unsupported")}
              </button>
              {row.mode === "transform" ? (
                <select
                  className="select !w-auto min-w-[180px]"
                  value={row.target}
                  onChange={(event) =>
                    onTargetChange(row.opIndex, parseCustomProto(event.target.value))
                  }
                >
                  {CUSTOM_PROTO_OPTIONS.map((option) => (
                    <option key={option} value={option}>
                      {option}
                    </option>
                  ))}
                </select>
              ) : null}
            </div>
          </div>
        ))}
      </div>
    </div>
  );

  const renderModelTableEditor = (
    useModelTable: boolean,
    models: CustomModelDraft[],
    onUseModelTableChange: (next: boolean) => void,
    onModelsChange: (next: CustomModelDraft[]) => void,
    idPrefix: string
  ) => (
    <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-900">{t("providers.model_table_title")}</div>
          <div className="mt-1 text-xs text-slate-500">{t("providers.model_table_hint")}</div>
        </div>
        <div className="flex items-center gap-2">
          <input
            id={`${idPrefix}-model-table-enabled`}
            type="checkbox"
            checked={useModelTable}
            onChange={(event) => onUseModelTableChange(event.target.checked)}
          />
          <label htmlFor={`${idPrefix}-model-table-enabled`} className="text-sm text-slate-700">
            {t("providers.model_table_enable")}
          </label>
        </div>
      </div>
      {useModelTable ? (
        <div className="mt-3 space-y-2">
          {models.map((row, index) => (
            <div
              key={`custom-model-${index}`}
              className="grid gap-2 rounded-xl border border-slate-200 bg-white p-3 md:grid-cols-[1fr_1fr_auto]"
            >
              <TextInput
                value={row.id}
                onChange={(next) => {
                  const copy = [...models];
                  copy[index] = { ...copy[index], id: next };
                  onModelsChange(copy);
                }}
                placeholder={t("providers.model_id")}
              />
              <TextInput
                value={row.displayName}
                onChange={(next) => {
                  const copy = [...models];
                  copy[index] = { ...copy[index], displayName: next };
                  onModelsChange(copy);
                }}
                placeholder={t("providers.model_display_name")}
              />
              <Button
                variant="danger"
                onClick={() => {
                  const copy = models.filter((_, i) => i !== index);
                  onModelsChange(copy);
                }}
              >
                {t("providers.model_remove")}
              </Button>
            </div>
          ))}
          <Button
            variant="neutral"
            onClick={() =>
              onModelsChange([
                ...models,
                {
                  id: "",
                  displayName: ""
                }
              ])
            }
          >
            {t("providers.model_add")}
          </Button>
        </div>
      ) : null}
    </div>
  );

  const renderConfigTab = () => {
    if (!selected) {
      return null;
    }
    const kind = kindFromConfig(selected.config_json);
    const customDraft =
      kind === "custom"
        ? customConfigDrafts[selected.name] ??
          extractCustomDraft(selected.config_json, selected.enabled)
        : null;

    if (kind === "custom" && customDraft) {
      return (
        <div className="space-y-4">
          <div className="grid gap-4 md:grid-cols-2">
            <div>
              <FieldLabel>{t("providers.custom_id")}</FieldLabel>
              <div className="mt-2">
                <TextInput
                  value={customDraft.id}
                  onChange={(next) => updateSelectedCustomDraft((draft) => ({ ...draft, id: next }))}
                />
              </div>
            </div>
            <div>
              <FieldLabel>{t("providers.base_url")}</FieldLabel>
              <div className="mt-2">
                <TextInput
                  value={customDraft.baseUrl}
                  onChange={(next) =>
                    updateSelectedCustomDraft((draft) => ({ ...draft, baseUrl: next }))
                  }
                />
              </div>
            </div>
            <div>
              <FieldLabel>{t("providers.custom_proto")}</FieldLabel>
              <select
                className="mt-2 select"
                value={customDraft.proto}
                onChange={(event) =>
                  updateSelectedCustomDraft((draft) => ({
                    ...draft,
                    proto: parseCustomProto(event.target.value)
                  }))
                }
              >
                {CUSTOM_PROTO_OPTIONS.map((option) => (
                  <option key={option} value={option}>
                    {option}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <FieldLabel>{t("providers.custom_count_tokens")}</FieldLabel>
              <select
                className="mt-2 select"
                value={customDraft.countTokens}
                onChange={(event) =>
                  updateSelectedCustomDraft((draft) => ({
                    ...draft,
                    countTokens: parseCountTokensMode(event.target.value)
                  }))
                }
              >
                <option value="upstream">upstream</option>
                <option value="tokenizers">tokenizers</option>
                <option value="tiktoken">tiktoken</option>
              </select>
            </div>
            <div className="md:col-span-2">
              <FieldLabel>{t("providers.json_param_mask")}</FieldLabel>
              <div className="mt-2">
                <TextArea
                  value={customDraft.jsonParamMaskText}
                  onChange={(next) =>
                    updateSelectedCustomDraft((draft) => ({ ...draft, jsonParamMaskText: next }))
                  }
                  rows={4}
                  placeholder={t("providers.json_param_mask_placeholder")}
                />
              </div>
              <div className="mt-1 text-xs text-slate-500">{t("providers.json_param_mask_hint")}</div>
            </div>
          </div>

          {renderDispatchEditor(
            customDraft.dispatchRows,
            (opIndex, mode) =>
              updateSelectedCustomDraft((draft) => ({
                ...draft,
                dispatchRows: draft.dispatchRows.map((row) =>
                  row.opIndex === opIndex ? { ...row, mode } : row
                )
              })),
            (opIndex, target) =>
              updateSelectedCustomDraft((draft) => ({
                ...draft,
                dispatchRows: draft.dispatchRows.map((row) =>
                  row.opIndex === opIndex ? { ...row, target } : row
                )
              })),
            () =>
              updateSelectedCustomDraft((draft) => ({
                ...draft,
                dispatchRows: buildDefaultDispatchRows(draft.proto)
              }))
          )}

          {renderModelTableEditor(
            customDraft.useModelTable,
            customDraft.models,
            (next) =>
              updateSelectedCustomDraft((draft) => ({
                ...draft,
                useModelTable: next
              })),
            (next) =>
              updateSelectedCustomDraft((draft) => ({
                ...draft,
                models: next
              })),
            `config-${selected.name}`
          )}

          <div>
            <Button onClick={() => void saveProvider()}>{t("common.save")}</Button>
          </div>
        </div>
      );
    }

    return (
      <div className="space-y-4">
        <div className="grid gap-4 md:grid-cols-2">
          {configFieldMap[kind].map((field) => {
            const fieldKey = field.key;
            const value = drafts[selected.name]?.[fieldKey] ?? "";
            const defaultValue = getConfigFieldDefault(kind, fieldKey);
            const displayValue = value || defaultValue || "";
            return (
              <div key={field.key} className={field.type === "textarea" ? "md:col-span-2" : ""}>
                <FieldLabel>{t(`providers.${fieldKey}`) || field.key}</FieldLabel>
                <div className="mt-2">
                  {field.type === "select" && field.options ? (
                    <select
                      className="select"
                      value={displayValue}
                      onChange={(event) =>
                        setDrafts((prev) => ({
                          ...prev,
                          [selected.name]: {
                            ...(prev[selected.name] ?? {}),
                            [fieldKey]: event.target.value
                          }
                        }))
                      }
                    >
                      {field.options.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.labelKey ? t(option.labelKey) : option.value}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <TextInput
                      value={value}
                      placeholder={!value && defaultValue ? defaultValue : undefined}
                      type={field.type === "number" ? "number" : field.type === "password" ? "password" : "text"}
                      onChange={(next) =>
                        setDrafts((prev) => ({
                          ...prev,
                          [selected.name]: {
                            ...(prev[selected.name] ?? {}),
                            [fieldKey]: next
                          }
                        }))
                      }
                    />
                  )}
                </div>
                {!value && defaultValue ? (
                  <div className="mt-1 text-xs text-slate-500">
                    {t("common.default_value")}: {defaultValue}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>

        <div>
          <Button onClick={() => void saveProvider()}>{t("common.save")}</Button>
        </div>
      </div>
    );
  };

  const renderCredentialsTab = () => {
    if (!selected) {
      return null;
    }
    const isClaudeCodeProvider = selected.name === "claudecode";
    return (
      <div className="space-y-5">
        <div className="rounded-2xl border border-slate-200 bg-white/60 p-4">
          <div className="grid gap-4 md:grid-cols-2">
            <div className="md:col-span-2">
              <div className="text-xs text-slate-500">
                {jsonCredentialKind ? t("credentials.import_mode_json") : t("credentials.import_mode_key")}
              </div>
            </div>
            {jsonCredentialKind ? (
              <div>
                <FieldLabel>{t("credentials.import_files")}</FieldLabel>
                <div className="mt-2 space-y-2">
                  <input
                    ref={credentialFileInputRef}
                    type="file"
                    multiple
                    accept="application/json,.json"
                    className="hidden"
                    onChange={(event) => setCredentialImportFiles(Array.from(event.target.files ?? []))}
                  />
                  <Button
                    variant="neutral"
                    onClick={() => {
                      if (credentialFileInputRef.current) {
                        credentialFileInputRef.current.value = "";
                        credentialFileInputRef.current.click();
                      }
                    }}
                  >
                    {t("credentials.import_files_button")}
                  </Button>
                  <div className="text-xs text-slate-500">
                    {credentialImportFiles.length === 0
                      ? t("credentials.import_files_empty")
                      : t("credentials.import_files_count", { count: String(credentialImportFiles.length) })}
                  </div>
                  {credentialImportFiles.length > 0 ? (
                    <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-600">
                      {credentialImportFiles.map((file) => file.name).join(", ")}
                    </div>
                  ) : null}
                </div>
              </div>
            ) : null}
            <div className={jsonCredentialKind ? "" : "md:col-span-2"}>
              <FieldLabel>{jsonCredentialKind ? t("credentials.import_json_text") : t("credentials.import_keys")}</FieldLabel>
              <div className="mt-2">
                <TextArea
                  value={credentialImportText}
                  onChange={setCredentialImportText}
                  rows={jsonCredentialKind ? 12 : 6}
                  placeholder={
                    jsonCredentialKind
                      ? t("credentials.import_json_placeholder")
                      : t("credentials.import_keys_placeholder")
                  }
                />
              </div>
            </div>
          </div>
          <div className="mt-4">
            <Button onClick={() => void runCredentialImport()} disabled={credentialImporting}>
              {credentialImporting ? t("common.loading") : t("credentials.import_run")}
            </Button>
          </div>
        </div>

        <div className="space-y-3">
          {credentialsLoading ? (
            <div className="text-sm text-slate-500">{t("common.loading")}</div>
          ) : credentials.length === 0 ? (
            <div className="text-sm text-slate-500">{t("common.empty")}</div>
          ) : (
            credentials.map((row) => {
              const displaySecret = unwrapCredentialSecretForDisplay(row.secret_json);
              const displaySecretText = JSON.stringify(displaySecret.payload, null, 2);
              const oneMFlags = isClaudeCodeProvider
                ? extractClaudeCodeOneMFlags(row.secret_json)
                : null;
              const oneMSonnetEnabled = oneMFlags?.enable_claude_1m_sonnet ?? true;
              const oneMOpusEnabled = oneMFlags?.enable_claude_1m_opus ?? true;
              const runtimeSummary =
                row.runtime_status?.summary ?? (row.enabled ? "active" : "disabled");
              const modelUnavailable = row.runtime_status?.model_unavailable ?? [];
              const credentialUnavailable = row.runtime_status?.credential_unavailable;
              const isPartialUnavailable = runtimeSummary === "partial_unavailable";
              const runtimeExpanded = Boolean(credentialRuntimeOpenIds[row.id]);
              const unavailableModelSet = new Set(modelUnavailable.map((item) => item.model));
              const knownModelSet = new Set<string>(selectedCustomKnownModels);
              for (const quotaRow of quotaRowsByCredentialId[row.id] ?? []) {
                knownModelSet.add(quotaRow.name);
              }
              for (const item of modelUnavailable) {
                knownModelSet.add(item.model);
              }
              const availableModels = Array.from(knownModelSet)
                .filter((name) => !unavailableModelSet.has(name))
                .sort((a, b) => a.localeCompare(b));
              return (
                <div key={row.id} className="rounded-2xl border border-slate-200 bg-white/70 p-4">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="text-sm font-semibold text-slate-900">
                      #{row.id} {row.name ?? ""}
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge active={row.enabled}>
                      {row.enabled ? t("common.enabled") : t("common.disabled")}
                    </Badge>
                    <span
                      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold ${runtimeSummaryClass(runtimeSummary)}`}
                    >
                      {t(`credentials.runtime_${runtimeSummary}`)}
                    </span>
                  </div>
                </div>
                {isClaudeCodeProvider && oneMFlags ? (
                  <div className="mt-3 rounded-xl border border-slate-200 bg-slate-50 p-3">
                    <div className="text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                      {t("credentials.one_m_title")}
                    </div>
                    <div className="mt-2 grid gap-3 md:grid-cols-2">
                      <div className="rounded-lg border border-slate-200 bg-white p-3">
                        <div className="text-sm font-medium text-slate-900">
                          {t("credentials.one_m_sonnet")}
                        </div>
                        <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-slate-600">
                          <Badge active={oneMSonnetEnabled}>
                            {oneMSonnetEnabled ? t("common.enabled") : t("common.disabled")}
                          </Badge>
                          <span>
                            {t("credentials.one_m_support")}:{" "}
                            {oneMSupportLabel(oneMFlags.supports_claude_1m_sonnet)}
                          </span>
                        </div>
                        <div className="mt-2">
                          <Button
                            variant="neutral"
                            disabled={credentialOneMSavingId === row.id}
                            onClick={() =>
                              void toggleClaudeCodeOneM(
                                row,
                                "enable_claude_1m_sonnet",
                                !oneMSonnetEnabled
                              )
                            }
                          >
                            {credentialOneMSavingId === row.id
                              ? t("common.loading")
                              : oneMSonnetEnabled
                                ? t("credentials.one_m_disable")
                                : t("credentials.one_m_enable")}
                          </Button>
                        </div>
                      </div>
                      <div className="rounded-lg border border-slate-200 bg-white p-3">
                        <div className="text-sm font-medium text-slate-900">
                          {t("credentials.one_m_opus")}
                        </div>
                        <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-slate-600">
                          <Badge active={oneMOpusEnabled}>
                            {oneMOpusEnabled ? t("common.enabled") : t("common.disabled")}
                          </Badge>
                          <span>
                            {t("credentials.one_m_support")}:{" "}
                            {oneMSupportLabel(oneMFlags.supports_claude_1m_opus)}
                          </span>
                        </div>
                        <div className="mt-2">
                          <Button
                            variant="neutral"
                            disabled={credentialOneMSavingId === row.id}
                            onClick={() =>
                              void toggleClaudeCodeOneM(
                                row,
                                "enable_claude_1m_opus",
                                !oneMOpusEnabled
                              )
                            }
                          >
                            {credentialOneMSavingId === row.id
                              ? t("common.loading")
                              : oneMOpusEnabled
                                ? t("credentials.one_m_disable")
                                : t("credentials.one_m_enable")}
                          </Button>
                        </div>
                      </div>
                    </div>
                  </div>
                ) : null}
                {(credentialUnavailable || modelUnavailable.length > 0) ? (
                  <div className="mt-2 space-y-1 text-xs text-slate-600">
                    {credentialUnavailable ? (
                      <div>
                        {t("credentials.credential_unavailable")}:
                        {" "}
                        {runtimeReasonText(credentialUnavailable.reason)}
                        {" · "}
                        {t("credentials.remaining_secs", { secs: String(credentialUnavailable.remaining_secs) })}
                      </div>
                    ) : null}
                    {isPartialUnavailable ? (
                      <div>
                        <button
                          type="button"
                          onClick={() => toggleCredentialRuntimeView(row.id)}
                          className="cursor-pointer text-xs font-semibold text-sky-700 hover:text-sky-900"
                        >
                          {runtimeExpanded
                            ? t("credentials.model_status_collapse")
                            : t("credentials.model_status_expand")}
                        </button>
                      </div>
                    ) : modelUnavailable.length > 0 ? (
                      <div className="space-y-1">
                        <div>{t("credentials.model_unavailable")} ({modelUnavailable.length})</div>
                        {modelUnavailable.map((item) => (
                          <div key={`${row.id}-${item.model}`} className="truncate">
                            {item.model}
                            {" · "}
                            {runtimeReasonText(item.reason)}
                            {" · "}
                            {t("credentials.remaining_secs", { secs: String(item.remaining_secs) })}
                          </div>
                        ))}
                      </div>
                    ) : null}
                  </div>
                ) : null}
                {isPartialUnavailable && runtimeExpanded ? (
                  <div className="mt-3 rounded-xl border border-slate-200 bg-white p-3">
                    <div className="grid gap-3 md:grid-cols-2">
                      <div className="space-y-1">
                        <div className="text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                          {t("credentials.model_unavailable")} ({modelUnavailable.length})
                        </div>
                        {modelUnavailable.length > 0 ? (
                          modelUnavailable.map((item) => (
                            <div
                              key={`runtime-unavailable-${row.id}-${item.model}`}
                              className="rounded-lg border border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-800"
                            >
                              {item.model}
                              {" · "}
                              {runtimeReasonText(item.reason)}
                              {" · "}
                              {t("credentials.remaining_secs", { secs: String(item.remaining_secs) })}
                            </div>
                          ))
                        ) : (
                          <div className="text-xs text-slate-500">{t("common.none")}</div>
                        )}
                      </div>
                      <div className="space-y-1">
                        <div className="text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                          {t("credentials.model_available")} ({availableModels.length})
                        </div>
                        {availableModels.length > 0 ? (
                          availableModels.map((name) => (
                            <div
                              key={`runtime-available-${row.id}-${name}`}
                              className="rounded-lg border border-emerald-200 bg-emerald-50 px-2 py-1 text-xs text-emerald-800"
                            >
                              {name}
                            </div>
                          ))
                        ) : (
                          <div className="text-xs text-slate-500">{t("credentials.model_available_unknown")}</div>
                        )}
                      </div>
                    </div>
                  </div>
                ) : null}
                <div
                  className={`mt-3 grid gap-2 ${supportsLiveUsage ? "sm:grid-cols-5" : "sm:grid-cols-4"}`}
                >
                  <Button variant="neutral" onClick={() => toggleCredentialView(row.id)}>
                    {credentialViewOpenIds[row.id]
                      ? t("credentials.hide_secret")
                      : t("credentials.view_secret")}
                  </Button>
                  <Button
                    variant="neutral"
                    onClick={() => {
                      if (credentialEditingId === row.id) {
                        cancelCredentialEdit();
                        return;
                      }
                      startCredentialEdit(row);
                    }}
                  >
                    {credentialEditingId === row.id ? t("common.cancel") : t("common.edit")}
                  </Button>
                  <Button variant="neutral" onClick={() => void toggleCredential(row)}>
                    {row.enabled ? t("common.disabled") : t("common.enabled")}
                  </Button>
                  <Button variant="danger" onClick={() => void deleteCredential(row.id)}>
                    {t("common.delete")}
                  </Button>
                  {supportsLiveUsage ? (
                    <Button
                      variant="neutral"
                      onClick={() => void loadCredentialQuota(row.id)}
                      disabled={quotaLoadingId === row.id}
                    >
                      {quotaLoadingId === row.id
                        ? t("common.loading")
                        : Object.prototype.hasOwnProperty.call(quotaRowsByCredentialId, row.id)
                          ? t("usage.live_refresh")
                          : t("usage.live_view_quota")}
                    </Button>
                  ) : null}
                </div>
                {Object.prototype.hasOwnProperty.call(quotaRowsByCredentialId, row.id) ? (
                  <div className="mt-3 overflow-hidden rounded-xl border border-slate-200 bg-white">
                    <div className="grid grid-cols-[minmax(0,2fr)_minmax(120px,1fr)_minmax(160px,1fr)] gap-2 border-b border-slate-200 bg-slate-50 px-4 py-3 text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                      <span>{t("usage.live_limit")}</span>
                      <span>{t("usage.live_percent")}</span>
                      <span>{t("usage.live_reset")}</span>
                    </div>
                    {quotaRowsByCredentialId[row.id].length > 0 ? (
                      <div className="divide-y divide-slate-100">
                        {quotaRowsByCredentialId[row.id].map((quotaRow) => (
                          <div
                            key={`${row.id}-${quotaRow.name}`}
                            className="grid grid-cols-[minmax(0,2fr)_minmax(120px,1fr)_minmax(160px,1fr)] gap-2 px-4 py-3 text-sm text-slate-700"
                          >
                            <span className="truncate font-medium text-slate-900">{quotaRow.name}</span>
                            <span>{formatUsagePercent(quotaRow.percent)}</span>
                            <span>{formatDateTime(quotaRow.resetAt)}</span>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="px-4 py-3 text-sm text-slate-500">{t("usage.live_no_limits")}</div>
                    )}
                  </div>
                ) : null}
                {credentialViewOpenIds[row.id] ? (
                  <div className="mt-3 max-w-full rounded-xl border border-slate-200 bg-white p-3">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                        {displaySecret.wrapperKey
                          ? `${t("credentials.secret_json")} (${displaySecret.wrapperKey})`
                          : t("credentials.secret_json")}
                      </div>
                      <Button
                        variant="neutral"
                        onClick={() => void copyCredentialSecret(row.id, displaySecretText)}
                      >
                        {credentialCopiedId === row.id ? t("common.copied") : t("common.copy")}
                      </Button>
                    </div>
                    <pre className="mt-2 max-h-[420px] max-w-full overflow-auto whitespace-pre-wrap break-all rounded-lg bg-slate-950 px-3 py-3 text-xs text-emerald-100">
                      {displaySecretText}
                    </pre>
                  </div>
                ) : null}
                {credentialEditingId === row.id ? (
                  <div className="mt-3 rounded-xl border border-slate-200 bg-white p-3">
                    <div className="grid gap-3">
                      <div>
                        <FieldLabel>{t("credentials.display_name")}</FieldLabel>
                        <div className="mt-2">
                          <TextInput value={credentialEditName} onChange={setCredentialEditName} />
                        </div>
                      </div>
                      <div>
                        <FieldLabel>{t("credentials.secret_json")}</FieldLabel>
                        <div className="mt-2">
                          <TextArea
                            value={credentialEditSecretJson}
                            onChange={setCredentialEditSecretJson}
                            rows={10}
                          />
                        </div>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Button
                          onClick={() => void saveCredentialEdit()}
                          disabled={credentialSavingId === row.id}
                        >
                          {credentialSavingId === row.id ? t("common.loading") : t("common.save")}
                        </Button>
                        <Button variant="neutral" onClick={cancelCredentialEdit}>
                          {t("common.cancel")}
                        </Button>
                      </div>
                    </div>
                  </div>
                ) : null}
                </div>
              );
            })
          )}
        </div>
      </div>
    );
  };

  const renderOauthTab = () => {
    if (!selected) {
      return null;
    }
    if (!supportsOAuth) {
      return <div className="text-sm text-slate-500">{t("oauth.unsupported")}</div>;
    }
    const oauthDefaults = oauthUiDefaults(selected.name);
    const defaultRedirectUri = oauthDefaults.redirectUri ?? undefined;
    const defaultScope = oauthDefaults.scope ?? undefined;
    const defaultProjectId =
      selected.name === "geminicli"
        ? t("oauth.default_project_auto_detect")
        : selected.name === "antigravity"
          ? t("oauth.default_project_auto_detect_or_random")
          : selected.name === "codex"
            ? t("oauth.default_not_required")
            : undefined;
    const defaultCallbackUrl =
      selected.name === "codex" ? t("oauth.default_not_required") : t("oauth.default_optional");
    const startRows = oauthStartResult ? scalarEntries(oauthStartResult) : [];
    const callbackRows = oauthCallbackResult ? scalarEntries(oauthCallbackResult) : [];

    return (
      <div className="space-y-5">
        <div className="grid gap-4 md:grid-cols-3">
          <div>
            <FieldLabel>{t("oauth.redirect_uri")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthStartParams.redirect_uri}
                placeholder={oauthStartParams.redirect_uri ? undefined : defaultRedirectUri}
                onChange={(value) =>
                  setOauthStartParams((prev) => ({ ...prev, redirect_uri: value }))
                }
              />
            </div>
            {oauthStartParams.redirect_uri ? null : defaultRedirectUri ? (
              <div className="mt-1 text-xs text-slate-500 break-all">
                {t("common.default_value")}: {defaultRedirectUri}
              </div>
            ) : null}
          </div>
          <div>
            <FieldLabel>{t("oauth.scope")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthStartParams.scope}
                placeholder={oauthStartParams.scope ? undefined : defaultScope}
                onChange={(value) =>
                  setOauthStartParams((prev) => ({ ...prev, scope: value }))
                }
              />
            </div>
            {oauthStartParams.scope ? null : defaultScope ? (
              <div className="mt-1 text-xs text-slate-500 break-all">
                {t("common.default_value")}: {defaultScope}
              </div>
            ) : null}
          </div>
          <div>
            <FieldLabel>{t("oauth.project_id")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthStartParams.project_id}
                placeholder={oauthStartParams.project_id ? undefined : defaultProjectId}
                onChange={(value) =>
                  setOauthStartParams((prev) => ({ ...prev, project_id: value }))
                }
              />
            </div>
            {oauthStartParams.project_id ? null : defaultProjectId ? (
              <div className="mt-1 text-xs text-slate-500 break-all">
                {t("common.default_value")}: {defaultProjectId}
              </div>
            ) : null}
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          {selected.name === "codex" ? (
            <>
              <Button onClick={() => void runOAuthStart("device_auth")}>
                {t("oauth.start_device_auth")}
              </Button>
              <Button variant="neutral" onClick={() => void runOAuthStart("authorization_code")}>
                {t("oauth.start_authorization_code")}
              </Button>
            </>
          ) : (
            <Button onClick={() => void runOAuthStart()}>{t("oauth.start")}</Button>
          )}
          {oauthStartResult?.auth_url ? (
            <Button
              variant="neutral"
              onClick={() =>
                window.open(String(oauthStartResult.auth_url), "_blank", "noopener,noreferrer")
              }
            >
              {t("oauth.open_auth")}
            </Button>
          ) : null}
        </div>

        {startRows.length > 0 ? (
          <div className="max-w-full rounded-xl border border-slate-200 bg-slate-50 p-4">
            <div className="grid gap-2 md:grid-cols-2">
              {startRows.map(([key, value]) => (
                <div key={key} className="min-w-0 text-sm text-slate-700">
                  <span className="font-semibold">{key}:</span>{" "}
                  <span className="break-all">{value}</span>
                </div>
              ))}
            </div>
          </div>
        ) : null}

        <div className="grid gap-4 md:grid-cols-2">
          <div>
            <FieldLabel>{t("oauth.state")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthCallbackParams.state}
                onChange={(value) =>
                  setOauthCallbackParams((prev) => ({ ...prev, state: value }))
                }
              />
            </div>
          </div>
          <div>
            <FieldLabel>{t("oauth.code")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthCallbackParams.code}
                onChange={(value) =>
                  setOauthCallbackParams((prev) => ({ ...prev, code: value }))
                }
              />
            </div>
          </div>
          <div className="md:col-span-2">
            <FieldLabel>{t("oauth.callback_url")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthCallbackParams.callback_url}
                placeholder={oauthCallbackParams.callback_url ? undefined : defaultCallbackUrl}
                onChange={(value) =>
                  setOauthCallbackParams((prev) => ({ ...prev, callback_url: value }))
                }
              />
            </div>
            {oauthCallbackParams.callback_url ? null : defaultCallbackUrl ? (
              <div className="mt-1 text-xs text-slate-500 break-all">
                {t("common.default_value")}: {defaultCallbackUrl}
              </div>
            ) : null}
          </div>
          <div>
            <FieldLabel>{t("oauth.project_id")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                value={oauthCallbackParams.project_id}
                placeholder={oauthCallbackParams.project_id ? undefined : defaultProjectId}
                onChange={(value) =>
                  setOauthCallbackParams((prev) => ({ ...prev, project_id: value }))
                }
              />
            </div>
            {oauthCallbackParams.project_id ? null : defaultProjectId ? (
              <div className="mt-1 text-xs text-slate-500 break-all">
                {t("common.default_value")}: {defaultProjectId}
              </div>
            ) : null}
          </div>
        </div>

        <div>
          <Button onClick={() => void runOAuthCallback()}>{t("oauth.callback")}</Button>
        </div>

        {callbackRows.length > 0 ? (
          <div className="max-w-full rounded-xl border border-slate-200 bg-slate-50 p-4">
            <div className="grid gap-2 md:grid-cols-2">
              {callbackRows.map(([key, value]) => (
                <div key={key} className="min-w-0 text-sm text-slate-700">
                  <span className="font-semibold">{key}:</span>{" "}
                  <span className="break-all">{value}</span>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    );
  };

  return (
    <div className="space-y-5">
      <Card
        title={t("providers.title")}
        subtitle={t("providers.subtitle")}
        action={
          <div className="flex gap-2">
            <Button variant="neutral" onClick={() => void loadProviders()} disabled={loading}>
              {t("common.refresh")}
            </Button>
            <Button variant="neutral" onClick={() => setCreatingCustom((prev) => !prev)}>
              {creatingCustom ? t("providers.create_custom_close") : t("providers.create_custom_open")}
            </Button>
          </div>
        }
      >
        {creatingCustom ? (
          <div className="mb-4 rounded-2xl border border-slate-200 bg-white/70 p-4">
            <div className="mb-3 text-xs text-slate-500">{t("providers.create_custom_hint")}</div>
            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <FieldLabel>{t("providers.custom_name")}</FieldLabel>
                <div className="mt-2">
                  <TextInput value={customName} onChange={setCustomName} placeholder="my-custom" />
                </div>
              </div>
              <div>
                <FieldLabel>{t("providers.custom_id")}</FieldLabel>
                <div className="mt-2">
                  <TextInput value={customId} onChange={setCustomId} placeholder="custom-my-custom" />
                </div>
              </div>
              <div>
                <FieldLabel>{t("providers.base_url")}</FieldLabel>
                <div className="mt-2">
                  <TextInput value={customBaseUrl} onChange={setCustomBaseUrl} placeholder="https://api.example.com" />
                </div>
              </div>
              <div>
                <FieldLabel>{t("providers.custom_proto")}</FieldLabel>
                <select
                  className="mt-2 select"
                  value={customProto}
                  onChange={(event) => {
                    const nextProto = parseCustomProto(event.target.value);
                    setCustomProto(nextProto);
                    setCustomDispatchRows(buildDefaultDispatchRows(nextProto));
                  }}
                >
                  {CUSTOM_PROTO_OPTIONS.map((option) => (
                    <option key={option} value={option}>
                      {option}
                    </option>
                  ))}
                </select>
              </div>
              <div>
                <FieldLabel>{t("providers.custom_count_tokens")}</FieldLabel>
                <select
                  className="mt-2 select"
                  value={customCountTokens}
                  onChange={(event) => setCustomCountTokens(event.target.value as CountTokensMode)}
                >
                  <option value="upstream">upstream</option>
                  <option value="tokenizers">tokenizers</option>
                  <option value="tiktoken">tiktoken</option>
                </select>
              </div>
              <div className="md:col-span-2">
                <FieldLabel>{t("providers.json_param_mask")}</FieldLabel>
                <div className="mt-2">
                  <TextArea
                    value={customJsonParamMaskText}
                    onChange={setCustomJsonParamMaskText}
                    rows={4}
                    placeholder={t("providers.json_param_mask_placeholder")}
                  />
                </div>
                <div className="mt-1 text-xs text-slate-500">{t("providers.json_param_mask_hint")}</div>
              </div>
            </div>
            <div className="mt-4">
              {renderDispatchEditor(
                customDispatchRows,
                (opIndex, mode) =>
                  setCustomDispatchRows((prev) =>
                    prev.map((row) => (row.opIndex === opIndex ? { ...row, mode } : row))
                  ),
                (opIndex, target) =>
                  setCustomDispatchRows((prev) =>
                    prev.map((row) => (row.opIndex === opIndex ? { ...row, target } : row))
                  ),
                () => setCustomDispatchRows(buildDefaultDispatchRows(customProto))
              )}
            </div>
            <div className="mt-4">
              {renderModelTableEditor(
                customUseModelTable,
                customModels,
                setCustomUseModelTable,
                setCustomModels,
                "create"
              )}
            </div>
            <div className="mt-4">
              <Button onClick={() => void createCustomProvider()} disabled={customCreating}>
                {customCreating ? t("common.loading") : t("providers.custom_create")}
              </Button>
            </div>
          </div>
        ) : null}

        {items.length === 0 ? (
          <div className="text-sm text-slate-500">{t("common.empty")}</div>
        ) : (
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {items.map((provider) => {
              const kind = kindFromConfig(provider.config_json);
              const isActive = provider.name === selectedName;
              const isToggling = providerTogglingName === provider.name;
              return (
                <div
                  key={provider.name}
                  onClick={() => setSelectedName(provider.name)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setSelectedName(provider.name);
                    }
                  }}
                  role="button"
                  tabIndex={0}
                  className={`provider-card ${isActive ? "provider-card-active" : ""}`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="text-left">
                      <div className="text-sm font-semibold text-slate-900">{provider.name}</div>
                      <div className="mt-1 text-xs text-slate-500">
                        {t("providers.kind")}: {kind}
                      </div>
                    </div>
                    <button
                      type="button"
                      className={`badge cursor-pointer ${provider.enabled ? "badge-active" : ""}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        void toggleProviderEnabled(provider);
                      }}
                      disabled={isToggling}
                    >
                      {isToggling
                        ? t("common.loading")
                        : provider.enabled
                          ? t("common.enabled")
                          : t("common.disabled")}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </Card>

      {selected ? (
        <Card
          title={t("providers.workspace_title", { provider: selected.name })}
          subtitle={`/admin/providers/${selected.name}`}
          action={
            selectedIsCustom ? (
              <Button
                variant="danger"
                onClick={() => void deleteCustomProvider()}
                disabled={customDeleting}
              >
                {customDeleting ? t("common.loading") : t("providers.custom_delete")}
              </Button>
            ) : undefined
          }
        >
          <div className="mb-4 flex flex-wrap gap-2">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                className={`btn ${workspaceTab === tab.id ? "btn-primary" : "btn-neutral"}`}
                onClick={() => setWorkspaceTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {workspaceTab === "config" ? renderConfigTab() : null}
          {workspaceTab === "credentials" ? renderCredentialsTab() : null}
          {workspaceTab === "oauth" ? renderOauthTab() : null}
        </Card>
      ) : null}
    </div>
  );
}

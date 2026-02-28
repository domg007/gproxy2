import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type Dispatch,
  type SetStateAction
} from "react";

import type {
  CredentialQueryRow,
  CredentialStatusQueryRow,
  ProviderQueryRow
} from "../../../lib/types";
import { formatAtForViewer } from "../../../lib/datetime";
import { Button, Input, Label, Select, TextArea } from "../../../components/ui";
import {
  availableBulkModes,
  buildBulkExportText,
  defaultBulkMode,
  formatUsagePercent,
  getChannelConfig,
  parseBulkCredentialText,
  type BulkCredentialImportEntry,
  type CredentialBulkMode,
  type CredentialsSubTab,
  type UsageDisplayKind,
  type UsageDisplayRow,
  type LiveUsageRow,
  ChannelCredentialSchema,
  CredentialFieldSchema,
  CredentialFieldValue,
  CredentialFormState,
  StatusFormState
} from "./index";

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;
type CredentialHealthKind = "healthy" | "partial" | "dead";

type CooldownItem = {
  model: string;
  untilUnixMs: number;
};

type OAuthReadableResult = {
  authUrl?: string;
  verificationUri?: string;
  userCode?: string;
  interval?: string;
  instructions?: string;
};

function parseOAuthReadableResult(raw: string): OAuthReadableResult | null {
  const text = raw.trim();
  if (!text) {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return null;
  }

  const object = parsed as Record<string, unknown>;
  const pick = (key: string): string | undefined => {
    const value = object[key];
    if (typeof value !== "string") {
      return undefined;
    }
    const trimmed = value.trim();
    return trimmed ? trimmed : undefined;
  };
  const pickNumberAsString = (key: string): string | undefined => {
    const value = object[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return String(value);
    }
    if (typeof value === "string") {
      const trimmed = value.trim();
      return trimmed ? trimmed : undefined;
    }
    return undefined;
  };

  return {
    authUrl: pick("auth_url"),
    verificationUri: pick("verification_uri"),
    userCode: pick("user_code"),
    interval: pickNumberAsString("interval"),
    instructions: pick("instructions")
  };
}

export function CredentialsTab({
  selectedProvider,
  credentialSchema,
  supportsUpstreamUsage,
  supportsOAuth,
  credentialRows,
  statusesByCredential,
  usageByCredential,
  liveUsageRowsByCredential,
  usageDisplayKindByCredential,
  usageDisplayRowsByCredential,
  usageLoadingByCredential,
  usageErrorByCredential,
  oauthStartQueryByCredential,
  setOauthStartQueryByCredential,
  oauthCallbackQueryByCredential,
  setOauthCallbackQueryByCredential,
  oauthResultByCredential,
  statusEditorCredentialId,
  setStatusEditorCredentialId,
  statusForm,
  setStatusForm,
  credentialForm,
  setCredentialForm,
  onEditCredential,
  onRemoveCredential,
  onToggleCredentialEnabled,
  onSetCredentialHealth,
  onQueryUpstreamUsage,
  onUpsertStatus,
  onRunCredentialOAuthStart,
  onRunCredentialOAuthCallback,
  onUpsertCredential,
  onUpsertCredentialsBatch,
  t
}: {
  selectedProvider: ProviderQueryRow | null;
  credentialSchema: ChannelCredentialSchema;
  supportsUpstreamUsage: boolean;
  supportsOAuth: boolean;
  credentialRows: CredentialQueryRow[];
  statusesByCredential: Map<number, CredentialStatusQueryRow[]>;
  usageByCredential: Record<number, string>;
  liveUsageRowsByCredential: Record<number, LiveUsageRow[]>;
  usageDisplayKindByCredential: Record<number, UsageDisplayKind>;
  usageDisplayRowsByCredential: Record<number, UsageDisplayRow[]>;
  usageLoadingByCredential: Record<number, boolean>;
  usageErrorByCredential: Record<number, string>;
  oauthStartQueryByCredential: Record<number, string>;
  setOauthStartQueryByCredential: Dispatch<SetStateAction<Record<number, string>>>;
  oauthCallbackQueryByCredential: Record<number, string>;
  setOauthCallbackQueryByCredential: Dispatch<SetStateAction<Record<number, string>>>;
  oauthResultByCredential: Record<number, string>;
  statusEditorCredentialId: number | null;
  setStatusEditorCredentialId: Dispatch<SetStateAction<number | null>>;
  statusForm: StatusFormState;
  setStatusForm: Dispatch<SetStateAction<StatusFormState>>;
  credentialForm: CredentialFormState;
  setCredentialForm: Dispatch<SetStateAction<CredentialFormState>>;
  onEditCredential: (row: CredentialQueryRow) => void;
  onRemoveCredential: (id: number) => void;
  onToggleCredentialEnabled: (row: CredentialQueryRow) => void;
  onSetCredentialHealth: (payload: {
    credentialId: number;
    statusId?: number;
    healthKind: CredentialHealthKind;
    healthJson: Record<string, unknown> | null;
    lastError?: string | null;
  }) => void;
  onQueryUpstreamUsage: (credentialId: number) => void;
  onUpsertStatus: () => void;
  onRunCredentialOAuthStart: (
    credentialId?: number,
    mode?: string,
    queryDefaults?: Record<string, string | null | undefined>
  ) => void;
  onRunCredentialOAuthCallback: (
    credentialId?: number,
    mode?: string,
    queryDefaults?: Record<string, string | null | undefined>
  ) => void;
  onUpsertCredential: () => void;
  onUpsertCredentialsBatch: (entries: BulkCredentialImportEntry[]) => void;
  t: TranslateFn;
}) {
  if (!selectedProvider) {
    return <p className="text-sm text-muted">{t("providers.needProvider")}</p>;
  }

  const resolveFieldLabel = (field: CredentialFieldSchema): string => {
    const key = `field.${field.key}`;
    const translated = t(key);
    return translated === key ? field.label : translated;
  };

  const setSecretField = (key: string, value: CredentialFieldValue) => {
    setCredentialForm((prev) => ({
      ...prev,
      secretValues: {
        ...prev.secretValues,
        [key]: value
      }
    }));
  };

  const channel = selectedProvider.channel.trim().toLowerCase();
  const GLOBAL_OAUTH_SLOT = 0;
  const oauthUi = getChannelConfig(channel)?.oauthUi;
  const oauthStartButtons = oauthUi?.startButtons ?? [{ labelKey: "providers.oauth.start" }];
  const oauthCallbackButtons = oauthUi?.callbackButtons ?? [{ labelKey: "providers.oauth.callback" }];
  const oauthCallbackUsesCustomFields = oauthCallbackButtons.some(
    (button) => Array.isArray(button.fields) && button.fields.length > 0
  );
  const bulkModes = useMemo(
    () => availableBulkModes(channel, credentialSchema, supportsOAuth),
    [channel, credentialSchema, supportsOAuth]
  );
  const [subTab, setSubTab] = useState<CredentialsSubTab>("single");
  const [bulkMode, setBulkMode] = useState<CredentialBulkMode>(
    defaultBulkMode(channel, credentialSchema, supportsOAuth)
  );
  const [expandedCooldownCredentialId, setExpandedCooldownCredentialId] = useState<number | null>(
    null
  );
  const [selectedCooldownKeysByCredential, setSelectedCooldownKeysByCredential] = useState<
    Record<number, string[]>
  >({});
  const [bulkInputText, setBulkInputText] = useState("");
  const [bulkExportText, setBulkExportText] = useState("");
  const [bulkError, setBulkError] = useState("");
  const bulkFileInputRef = useRef<HTMLInputElement | null>(null);
  const oauthReadableResult = useMemo(
    () => parseOAuthReadableResult(oauthResultByCredential[GLOBAL_OAUTH_SLOT] ?? ""),
    [oauthResultByCredential]
  );
  const oauthStartQuery = oauthStartQueryByCredential[GLOBAL_OAUTH_SLOT] ?? "";
  const oauthCallbackQuery = oauthCallbackQueryByCredential[GLOBAL_OAUTH_SLOT] ?? "";
  const oauthRawResult = oauthResultByCredential[GLOBAL_OAUTH_SLOT] ?? "";
  const oauthOpenUrl = oauthReadableResult?.authUrl ?? oauthReadableResult?.verificationUri;

  useEffect(() => {
    setSubTab("single");
    setBulkMode(defaultBulkMode(channel, credentialSchema, supportsOAuth));
    setBulkInputText("");
    setBulkExportText("");
    setBulkError("");
    setExpandedCooldownCredentialId(null);
    setSelectedCooldownKeysByCredential({});
  }, [channel, credentialSchema, supportsOAuth]);

  const normalizeHealthKind = (value: string | undefined): CredentialHealthKind => {
    switch (value?.trim().toLowerCase()) {
      case "dead":
        return "dead";
      case "partial":
        return "partial";
      default:
        return "healthy";
    }
  };

  const parseCooldowns = (status: CredentialStatusQueryRow | undefined): CooldownItem[] => {
    if (!status || normalizeHealthKind(status.health_kind) !== "partial") {
      return [];
    }
    const value = status.health_json;
    if (!value || typeof value !== "object") {
      return [];
    }
    const source = Array.isArray((value as { models?: unknown }).models)
      ? ((value as { models: unknown[] }).models ?? [])
      : Array.isArray(value)
        ? value
        : [];
    const result: CooldownItem[] = [];
    for (const item of source) {
      if (!item || typeof item !== "object" || Array.isArray(item)) {
        continue;
      }
      const model =
        typeof (item as { model?: unknown }).model === "string"
          ? (item as { model: string }).model.trim()
          : "";
      const untilRaw = (item as { until_unix_ms?: unknown }).until_unix_ms;
      const untilUnixMs =
        typeof untilRaw === "number" && Number.isFinite(untilRaw) ? Math.floor(untilRaw) : NaN;
      if (model && Number.isFinite(untilUnixMs)) {
        result.push({ model, untilUnixMs });
      }
    }
    return result;
  };

  const healthLabel = (kind: CredentialHealthKind): string => {
    if (kind === "dead") {
      return t("providers.health.dead");
    }
    if (kind === "partial") {
      return t("providers.health.partial");
    }
    return t("providers.health.healthy");
  };

  const cooldownKey = (item: CooldownItem): string => `${item.model}::${item.untilUnixMs}`;

  const bulkPlaceholder = useMemo(() => {
    if (bulkMode === "keys") {
      return `${t("providers.bulk.placeholder.keyLineA")}\n${t("providers.bulk.placeholder.keyLineB")}`;
    }
    if (bulkMode === "claudecode_cookie") {
      return `${t("providers.bulk.placeholder.cookieLineA")}\n${t("providers.bulk.placeholder.cookieLineB")}`;
    }
    const sample: Record<string, unknown> = {
      name: "credential-1",
      enabled: true
    };
    for (const field of credentialSchema.fields) {
      if (field.type === "integer") {
        sample[field.key] = 0;
      } else if (field.type === "boolean") {
        sample[field.key] = true;
      } else if (field.type === "optional_boolean") {
        sample[field.key] = true;
      } else {
        sample[field.key] = `${field.key}-value`;
      }
    }
    return JSON.stringify(sample);
  }, [bulkMode, credentialSchema.fields, t]);

  const formatWindowLabel = (window: UsageDisplayRow["window"]): string => {
    if (window === "primary") {
      return t("providers.usage.window_primary");
    }
    if (window === "secondary") {
      return t("providers.usage.window_secondary");
    }
    if (window === "code_review") {
      return t("providers.usage.window_code_review");
    }
    if (window === "5h") {
      return t("providers.usage.window_5h");
    }
    if (window === "1d") {
      return t("providers.usage.window_1d");
    }
    if (window === "1w") {
      return t("providers.usage.window_1w");
    }
    return t("providers.usage.window_sum");
  };

  const resolveUsageGroupLabel = (label: string): string => {
    if (label === "all") {
      return t("providers.usage.group_all");
    }
    if (label === "haiku") {
      return t("providers.usage.group_haiku");
    }
    if (label === "sonnet") {
      return t("providers.usage.group_sonnet");
    }
    if (label === "opus") {
      return t("providers.usage.group_opus");
    }
    return label;
  };

  const resolveLiveLimitLabel = (label: string): string => {
    const key = `providers.usage.live_key.${label}`;
    const translated = t(key);
    return translated === key ? label : translated;
  };

  const runBulkImport = () => {
    try {
      const entries = parseBulkCredentialText({
        channel,
        schema: credentialSchema,
        mode: bulkMode,
        rawText: bulkInputText
      });
      setBulkError("");
      onUpsertCredentialsBatch(entries);
    } catch (error) {
      setBulkError(error instanceof Error ? error.message : String(error));
    }
  };

  const runBulkExport = () => {
    const text = buildBulkExportText({
      channel,
      schema: credentialSchema,
      mode: bulkMode,
      credentialRows
    });
    setBulkExportText(text);
  };

  const openBulkImportFilePicker = () => {
    bulkFileInputRef.current?.click();
  };

  const onBulkImportFileChange = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) {
      return;
    }

    try {
      const text = await file.text();
      setBulkInputText(text);
      setBulkError("");
    } catch (error) {
      setBulkError(error instanceof Error ? error.message : String(error));
    }
  };

  const runBulkExportFile = () => {
    const text = buildBulkExportText({
      channel,
      schema: credentialSchema,
      mode: bulkMode,
      credentialRows
    });
    setBulkExportText(text);

    if (!text.trim()) {
      setBulkError(t("providers.bulk.emptyExport"));
      return;
    }

    const extension = bulkMode === "json" ? "jsonl" : "txt";
    const stamp = new Date().toISOString().replace(/[:.]/g, "-");
    const filename = `gproxy-credentials-${channel}-${stamp}.${extension}`;
    const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(url);
  };

  const readQueryParam = (rawQuery: string, key: string): string => {
    const input = rawQuery.trim();
    const params = new URLSearchParams(input.startsWith("?") ? input.slice(1) : input);
    return params.get(key) ?? "";
  };

  const updateQueryParam = (rawQuery: string, key: string, value: string): string => {
    const input = rawQuery.trim();
    const params = new URLSearchParams(input.startsWith("?") ? input.slice(1) : input);
    const trimmed = value.trim();
    if (trimmed) {
      params.set(key, trimmed);
    } else {
      params.delete(key);
    }
    const query = params.toString();
    return query ? `?${query}` : "";
  };

  const setOAuthStartParam = (key: string, value: string) => {
    setOauthStartQueryByCredential((prev) => ({
      ...prev,
      [GLOBAL_OAUTH_SLOT]: updateQueryParam(prev[GLOBAL_OAUTH_SLOT] ?? "", key, value)
    }));
  };

  const setOAuthCallbackParam = (key: string, value: string) => {
    setOauthCallbackQueryByCredential((prev) => ({
      ...prev,
      [GLOBAL_OAUTH_SLOT]: updateQueryParam(prev[GLOBAL_OAUTH_SLOT] ?? "", key, value)
    }));
  };

  const renderOAuthField = (
    kind: "start" | "callback",
    field: string,
    rawQuery: string
  ) => {
    const labelKey = `field.${field}`;
    const translated = t(labelKey);
    const placeholder =
      kind === "start"
        ? oauthUi?.startDefaults?.[field]
        : oauthUi?.callbackDefaults?.[field];
    return (
      <div key={`${kind}-${field}`}>
        <Label>{translated === labelKey ? field : translated}</Label>
        <Input
          value={readQueryParam(rawQuery, field)}
          onChange={(value) =>
            kind === "start"
              ? setOAuthStartParam(field, value)
              : setOAuthCallbackParam(field, value)
          }
          placeholder={placeholder}
        />
      </div>
    );
  };

  const renderCredentialField = (field: CredentialFieldSchema) => {
    const value = credentialForm.secretValues[field.key];
    if (field.type === "boolean") {
      return (
        <div key={field.key} className="flex items-end gap-2 pb-2">
          <input
            id={`credential-${field.key}`}
            type="checkbox"
            checked={value === true}
            onChange={(event) => setSecretField(field.key, event.target.checked)}
          />
          <label htmlFor={`credential-${field.key}`} className="text-sm text-muted">
            {resolveFieldLabel(field)}
          </label>
        </div>
      );
    }
    if (field.type === "optional_boolean") {
      const selected = value === true ? "true" : value === false ? "false" : "";
      return (
        <div key={field.key}>
          <Label>{resolveFieldLabel(field)}</Label>
          <Select
            value={selected}
            onChange={(next) =>
              setSecretField(
                field.key,
                next === "true" ? true : next === "false" ? false : null
              )
            }
            options={[
              { value: "", label: t("common.unset") },
              { value: "true", label: t("common.true") },
              { value: "false", label: t("common.false") }
            ]}
          />
        </div>
      );
    }
    return (
      <div key={field.key}>
        <Label>{resolveFieldLabel(field)}</Label>
        <Input
          value={typeof value === "string" ? value : ""}
          onChange={(next) => setSecretField(field.key, next)}
          placeholder={field.placeholder}
        />
      </div>
    );
  };

  return (
    <div className="space-y-4">
      <div className="grid gap-3 md:grid-cols-2">
        {credentialRows.map((row) => {
          const statusList = statusesByCredential.get(row.id) ?? [];
          const primaryStatus = statusList[0];
          const healthKind = normalizeHealthKind(primaryStatus?.health_kind);
          const cooldowns = parseCooldowns(primaryStatus);
          const selectedCooldownKeys = new Set(
            selectedCooldownKeysByCredential[row.id] ?? []
          );
          const selectedCooldowns = cooldowns.filter((item) =>
            selectedCooldownKeys.has(cooldownKey(item))
          );
          const showCooldowns =
            expandedCooldownCredentialId === row.id && healthKind === "partial";
          const usageContent = usageByCredential[row.id] ?? "";
          const liveRows = liveUsageRowsByCredential[row.id] ?? [];
          const usageDisplayKind = usageDisplayKindByCredential[row.id] ?? "calls";
          const usageDisplayRows = usageDisplayRowsByCredential[row.id] ?? [];
          const usageLoading = Boolean(usageLoadingByCredential[row.id]);
          const usageError = usageErrorByCredential[row.id];
          const showStatusEditor = statusEditorCredentialId === row.id;
          const applyCooldownDeletion = (targets: CooldownItem[]) => {
            if (targets.length === 0) {
              return;
            }
            const targetKeys = new Set(targets.map((item) => cooldownKey(item)));
            const nextModels = cooldowns.filter((item) => !targetKeys.has(cooldownKey(item)));
            onSetCredentialHealth({
              credentialId: row.id,
              statusId: primaryStatus?.id,
              healthKind: nextModels.length > 0 ? "partial" : "healthy",
              healthJson:
                nextModels.length > 0
                  ? {
                      models: nextModels.map((cooldown) => ({
                        model: cooldown.model,
                        until_unix_ms: cooldown.untilUnixMs
                      }))
                    }
                  : null,
              lastError: nextModels.length > 0 ? primaryStatus?.last_error : null
            });
            setSelectedCooldownKeysByCredential((prev) => {
              const next = { ...prev };
              delete next[row.id];
              return next;
            });
            if (nextModels.length === 0) {
              setExpandedCooldownCredentialId(null);
            }
          };
          return (
            <div key={row.id} className="provider-card space-y-3">
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold text-text">
                    {row.name ?? t("providers.credentialUnnamed")}
                  </div>
                  <div className="truncate text-xs text-muted">#{row.id}</div>
                </div>
                <div className="flex items-center gap-2">
                  <Button
                    variant={
                      healthKind === "dead"
                        ? "danger"
                        : healthKind === "partial"
                          ? "neutral"
                          : "primary"
                    }
                    onClick={() => {
                      if (healthKind === "partial") {
                        setExpandedCooldownCredentialId((prev) =>
                          prev === row.id ? null : row.id
                        );
                        return;
                      }
                      if (healthKind === "dead") {
                        onSetCredentialHealth({
                          credentialId: row.id,
                          statusId: primaryStatus?.id,
                          healthKind: "healthy",
                          healthJson: null,
                          lastError: null
                        });
                        return;
                      }
                      onSetCredentialHealth({
                        credentialId: row.id,
                        statusId: primaryStatus?.id,
                        healthKind: "dead",
                        healthJson: null,
                        lastError: "manually_marked_unavailable"
                      });
                    }}
                  >
                    {healthLabel(healthKind)}
                  </Button>
                  <Button
                    variant={row.enabled ? "primary" : "neutral"}
                    onClick={() => onToggleCredentialEnabled(row)}
                  >
                    {row.enabled ? t("common.enabled") : t("common.disabled")}
                  </Button>
                </div>
              </div>

              <div className="flex flex-wrap gap-2">
                <Button variant="neutral" onClick={() => onEditCredential(row)}>
                  {t("common.edit")}
                </Button>
                <Button variant="danger" onClick={() => onRemoveCredential(row.id)}>
                  {t("common.delete")}
                </Button>
                {supportsUpstreamUsage ? (
                  <Button
                    variant="neutral"
                    onClick={() => onQueryUpstreamUsage(row.id)}
                    disabled={usageLoading}
                  >
                    {usageLoading ? t("common.loading") : t("providers.usage.fetch")}
                  </Button>
                ) : null}
              </div>

              {showCooldowns ? (
                <div className="space-y-2 rounded-lg border border-border px-3 py-2">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                      {t("providers.health.cooldowns")}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="neutral"
                        disabled={selectedCooldowns.length === 0}
                        onClick={() => applyCooldownDeletion(selectedCooldowns)}
                      >
                        {t("providers.health.deleteSelected")}
                      </Button>
                      <Button
                        variant="danger"
                        disabled={cooldowns.length === 0}
                        onClick={() => applyCooldownDeletion(cooldowns)}
                      >
                        {t("providers.health.deleteAll")}
                      </Button>
                    </div>
                  </div>
                  {cooldowns.length === 0 ? (
                    <div className="text-xs text-muted">{t("providers.health.noCooldowns")}</div>
                  ) : (
                    cooldowns.map((item) => (
                      <div
                        key={`${row.id}-${item.model}-${item.untilUnixMs}`}
                        className="flex items-center justify-between gap-2 rounded border border-border px-2 py-1"
                      >
                        <label className="flex min-w-0 items-center gap-2 text-xs text-text">
                          <input
                            type="checkbox"
                            checked={selectedCooldownKeys.has(cooldownKey(item))}
                            onChange={(event) => {
                              setSelectedCooldownKeysByCredential((prev) => {
                                const current = new Set(prev[row.id] ?? []);
                                const key = cooldownKey(item);
                                if (event.target.checked) {
                                  current.add(key);
                                } else {
                                  current.delete(key);
                                }
                                return {
                                  ...prev,
                                  [row.id]: Array.from(current)
                                };
                              });
                            }}
                          />
                          <span className="font-semibold">{item.model}</span>{" "}
                          <span className="text-muted">
                            ({new Date(item.untilUnixMs).toLocaleString()})
                          </span>
                        </label>
                      </div>
                    ))
                  )}
                </div>
              ) : null}

              {showStatusEditor ? (
                <div className="space-y-2 rounded-lg border border-border p-3">
                  <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                    {t("providers.status.editor", { id: row.id })}
                  </div>
                  <div className="grid gap-2 md:grid-cols-2">
                    <div>
                      <Label>{t("field.idOptional")}</Label>
                      <Input value={statusForm.id} onChange={(v) => setStatusForm((p) => ({ ...p, id: v }))} />
                    </div>
                    <div>
                      <Label>{t("field.health_kind")}</Label>
                      <Input
                        value={statusForm.healthKind}
                        onChange={(v) => setStatusForm((p) => ({ ...p, healthKind: v }))}
                      />
                    </div>
                    <div>
                      <Label>{t("field.checked_at_unix_ms")}</Label>
                      <Input
                        value={statusForm.checkedAtUnixMs}
                        onChange={(v) => setStatusForm((p) => ({ ...p, checkedAtUnixMs: v }))}
                      />
                    </div>
                    <div>
                      <Label>{t("field.last_error")}</Label>
                      <Input
                        value={statusForm.lastError}
                        onChange={(v) => setStatusForm((p) => ({ ...p, lastError: v }))}
                      />
                    </div>
                    <div className="md:col-span-2">
                      <Label>{t("field.health_json")}</Label>
                      <TextArea
                        rows={4}
                        value={statusForm.healthJson}
                        onChange={(v) => setStatusForm((p) => ({ ...p, healthJson: v }))}
                      />
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <Button onClick={onUpsertStatus}>{t("common.save")}</Button>
                    <Button variant="neutral" onClick={() => setStatusEditorCredentialId(null)}>
                      {t("common.close")}
                    </Button>
                  </div>
                </div>
              ) : null}

              {supportsUpstreamUsage && (usageContent || liveRows.length > 0 || usageDisplayRows.length > 0 || usageError) ? (
                <div className="space-y-1">
                  <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                    {t("providers.section.usage")}
                  </div>
                  {liveRows.length > 0 ? (
                    <div className="overflow-hidden rounded-lg border border-border">
                      <div className="grid grid-cols-[minmax(0,2fr)_minmax(90px,1fr)_minmax(160px,1fr)] gap-2 border-b border-border bg-card px-3 py-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                        <span>{t("providers.usage.live_limit")}</span>
                        <span>{t("providers.usage.live_percent")}</span>
                        <span>{t("providers.usage.live_reset")}</span>
                      </div>
                      <div className="divide-y divide-border">
                        {liveRows.map((item) => (
                          <div
                            key={`${row.id}-usage-live-${item.name}-${item.resetAt ?? "none"}`}
                            className="grid grid-cols-[minmax(0,2fr)_minmax(90px,1fr)_minmax(160px,1fr)] gap-2 px-3 py-2 text-xs text-text"
                          >
                            <span className="truncate">{resolveLiveLimitLabel(item.name)}</span>
                            <span>{formatUsagePercent(item.percent)}</span>
                            <span>{item.resetAt === null ? "-" : formatAtForViewer(item.resetAt)}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : (
                    usageContent ? (
                      <div className="text-xs text-muted">{t("providers.usage.live_no_limits")}</div>
                    ) : null
                  )}

                  {usageDisplayRows.length > 0 ? (
                    (() => {
                      const preferredWindowOrder: UsageDisplayRow["window"][] =
                        channel === "codex"
                          ? ["primary", "secondary", "code_review"]
                          : ["5h", "1d", "1w", "sum"];
                      const presentWindowSet = new Set(
                        usageDisplayRows.map((item) => item.window)
                      );
                      const windows = preferredWindowOrder.filter((window) =>
                        presentWindowSet.has(window)
                      );
                      const byLabel = new Map<
                        string,
                        Partial<Record<UsageDisplayRow["window"], UsageDisplayRow>>
                      >();
                      for (const item of usageDisplayRows) {
                        const current = byLabel.get(item.label) ?? {};
                        current[item.window] = item;
                        byLabel.set(item.label, current);
                      }
                      const labels = Array.from(byLabel.keys()).sort((a, b) => a.localeCompare(b));

                      return (
                        <div className="space-y-2">
                          {usageDisplayKind === "tokens" ? (
                            <div className="text-xs text-muted">
                              {t("providers.usage.calls")}/{t("providers.usage.tokens_input")}/
                              {t("providers.usage.tokens_output")}/{t("providers.usage.tokens_cache")}/
                              {t("providers.usage.tokens_total")}
                            </div>
                          ) : null}
                          <div className="overflow-x-auto rounded-lg border border-border">
                            <table className="min-w-[980px] w-full border-collapse text-xs">
                              <thead>
                                <tr className="border-b border-border bg-card text-muted">
                                  <th className="px-3 py-2 text-left font-semibold uppercase tracking-[0.08em]">
                                    {t("providers.usage.label")}
                                  </th>
                                  {windows.map((window) => (
                                    <th
                                      key={`usage-head-${window}`}
                                      className="px-3 py-2 text-left font-semibold uppercase tracking-[0.08em]"
                                    >
                                      {formatWindowLabel(window)}
                                    </th>
                                  ))}
                                </tr>
                              </thead>
                              <tbody>
                                {labels.map((label) => {
                                  const rowByWindow = byLabel.get(label) ?? {};
                                  return (
                                    <tr key={`${row.id}-usage-row-${label}`} className="border-b border-border last:border-b-0">
                                      <td className="px-3 py-2 font-semibold text-text">
                                        {resolveUsageGroupLabel(label)}
                                      </td>
                                      {windows.map((window) => {
                                        const item = rowByWindow[window];
                                        if (!item) {
                                          return (
                                            <td key={`${row.id}-usage-cell-${label}-${window}`} className="px-3 py-2 text-muted">
                                              -
                                            </td>
                                          );
                                        }
                                        const rangeText = `${formatAtForViewer(item.fromUnixMs)} - ${formatAtForViewer(item.toUnixMs)}`;
                                        const cellText =
                                          usageDisplayKind === "tokens"
                                            ? `${item.calls}/${item.inputTokens}/${item.outputTokens}/${item.cacheTokens}/${item.totalTokens}`
                                            : `${item.calls}`;
                                        return (
                                          <td
                                            key={`${row.id}-usage-cell-${label}-${window}`}
                                            className="px-3 py-2 text-text whitespace-nowrap"
                                            title={rangeText}
                                          >
                                            {cellText}
                                          </td>
                                        );
                                      })}
                                    </tr>
                                  );
                                })}
                              </tbody>
                            </table>
                          </div>
                        </div>
                      );
                    })()
                  ) : (
                    usageContent ? (
                      <div className="text-xs text-muted">{t("providers.usage.no_calls")}</div>
                    ) : null
                  )}

                  {usageError ? (
                    <div className="text-xs text-amber-700">{usageError}</div>
                  ) : null}

                  {usageContent ? (
                    <details className="rounded-lg border border-border px-3 py-2">
                      <summary className="cursor-pointer text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                        {t("providers.usage.raw")}
                      </summary>
                      <div className="mt-2">
                        <TextArea value={usageContent} rows={8} readOnly onChange={() => {}} />
                      </div>
                    </details>
                  ) : null}
                </div>
              ) : null}

            </div>
          );
        })}
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className={`workspace-tab ${subTab === "single" ? "workspace-tab-active" : ""}`}
          onClick={() => setSubTab("single")}
        >
          {t("providers.subtab.single")}
        </button>
        <button
          type="button"
          className={`workspace-tab ${subTab === "bulk" ? "workspace-tab-active" : ""}`}
          onClick={() => setSubTab("bulk")}
        >
          {t("providers.subtab.bulk")}
        </button>
      </div>

      {subTab === "single" ? (
        <>
          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <Label>{t("field.id")}</Label>
              <Input value={credentialForm.id} onChange={(v) => setCredentialForm((p) => ({ ...p, id: v }))} />
            </div>
            <div>
              <Label>{t("field.nameOptional")}</Label>
              <Input
                value={credentialForm.name}
                onChange={(v) => setCredentialForm((p) => ({ ...p, name: v }))}
              />
            </div>
            {credentialSchema.fields.map((field) => renderCredentialField(field))}
          </div>

          <div>
            <Button onClick={onUpsertCredential}>{t("common.save")}</Button>
          </div>

          {supportsOAuth ? (
            <div className="provider-card space-y-2">
              <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                {t("providers.section.oauth")}
              </div>
              {oauthUi?.startFields.map((field) => renderOAuthField("start", field, oauthStartQuery))}
              <div className="flex flex-wrap gap-2">
                {oauthStartButtons.map((button) => (
                  <Button
                    key={button.labelKey}
                    variant={button.mode ? "neutral" : "primary"}
                    onClick={() =>
                      onRunCredentialOAuthStart(undefined, button.mode, button.queryDefaults)
                    }
                  >
                    {t(button.labelKey)}
                  </Button>
                ))}
                {oauthOpenUrl ? (
                  <a
                    className="btn btn-primary inline-flex"
                    href={oauthOpenUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t("providers.oauth.openAuthUrl")}
                  </a>
                ) : null}
                {!oauthCallbackUsesCustomFields
                  ? oauthCallbackButtons.map((button) => (
                      <Button
                        key={button.labelKey}
                        variant={button.mode ? "neutral" : "primary"}
                        onClick={() =>
                          onRunCredentialOAuthCallback(undefined, button.mode, button.queryDefaults)
                        }
                      >
                        {t(button.labelKey)}
                      </Button>
                    ))
                  : null}
              </div>
              {!oauthCallbackUsesCustomFields
                ? oauthUi?.callbackFields.map((field) =>
                    renderOAuthField("callback", field, oauthCallbackQuery)
                  )
                : null}
              {oauthCallbackUsesCustomFields ? (
                <div className="space-y-2">
                  {oauthCallbackButtons.map((button) => {
                    const fields = button.fields ?? oauthUi?.callbackFields ?? [];
                    return (
                      <div
                        key={`callback-${button.labelKey}`}
                        className="space-y-2 rounded-lg border border-border p-3"
                      >
                        <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                          {t(button.labelKey)}
                        </div>
                        {fields.map((field) => renderOAuthField("callback", field, oauthCallbackQuery))}
                        <Button
                          variant={button.mode ? "neutral" : "primary"}
                          onClick={() =>
                            onRunCredentialOAuthCallback(undefined, button.mode, button.queryDefaults)
                          }
                        >
                          {t(button.labelKey)}
                        </Button>
                      </div>
                    );
                  })}
                </div>
              ) : null}
              {oauthRawResult ? (
                <div className="space-y-2 rounded-lg border border-border p-3">
                  <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                    {t("providers.oauth.response")}
                  </div>
                  <TextArea value={oauthRawResult} rows={10} readOnly onChange={() => {}} />
                </div>
              ) : null}
            </div>
          ) : null}
        </>
      ) : (
        <div className="space-y-3 rounded-xl border border-border p-3">
          <div className="text-sm text-muted">{t("providers.bulk.hint")}</div>

          {bulkModes.length > 1 ? (
            <div>
              <Label>{t("providers.bulk.mode")}</Label>
              <Select
                value={bulkMode}
                onChange={(value) => {
                  setBulkMode(value as CredentialBulkMode);
                  setBulkError("");
                }}
                options={bulkModes.map((mode) => ({
                  value: mode,
                  label: t(`providers.bulk.mode.${mode}`)
                }))}
              />
            </div>
          ) : null}

          <div>
            <Label>{t("providers.bulk.input")}</Label>
            <TextArea
              rows={10}
              value={bulkInputText}
              onChange={(value) => {
                setBulkInputText(value);
                setBulkError("");
              }}
              placeholder={bulkPlaceholder}
            />
          </div>

          {bulkError ? <div className="text-sm text-red-500">{bulkError}</div> : null}

          <div className="flex flex-wrap gap-2">
            <Button onClick={runBulkImport}>{t("providers.bulk.import")}</Button>
            <Button variant="neutral" onClick={runBulkExport}>
              {t("providers.bulk.export")}
            </Button>
            <Button variant="neutral" onClick={() => setBulkInputText("")}>
              {t("providers.bulk.clearInput")}
            </Button>
            {bulkMode === "json" ? (
              <>
                <Button variant="neutral" onClick={openBulkImportFilePicker}>
                  {t("providers.bulk.importFile")}
                </Button>
                <Button variant="neutral" onClick={runBulkExportFile}>
                  {t("providers.bulk.exportFile")}
                </Button>
              </>
            ) : null}
          </div>

          {bulkMode === "json" ? (
            <input
              ref={bulkFileInputRef}
              type="file"
              accept=".json,.jsonl,application/json,text/plain"
              className="hidden"
              onChange={onBulkImportFileChange}
            />
          ) : null}

          <div>
            <Label>{t("providers.bulk.exportData")}</Label>
            <TextArea
              rows={10}
              value={bulkExportText}
              onChange={() => {}}
              readOnly
              placeholder={t("providers.bulk.exportPlaceholder")}
            />
          </div>
        </div>
      )}
    </div>
  );
}

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
import { Button, Input, Label, Select, TextArea } from "../../../components/ui";
import { CredentialBulkSection } from "./credentials-tab/CredentialBulkSection";
import { CredentialCardsSection } from "./credentials-tab/CredentialCardsSection";
import { CredentialSingleSection } from "./credentials-tab/CredentialSingleSection";
import { CredentialsSubTabs } from "./credentials-tab/CredentialsSubTabs";
import type { CooldownItem, CredentialHealthKind, TranslateFn } from "./credentials-tab/shared";
import {
  availableBulkModes,
  buildBulkExportText,
  defaultBulkMode,
  getChannelConfig,
  parseBulkCredentialText,
  type BulkCredentialImportEntry,
  type CredentialBulkMode,
  type CredentialsSubTab,
  type LiveUsageRow,
  type UsageDisplayKind,
  type UsageDisplayRow,
  ChannelCredentialSchema,
  CredentialFieldSchema,
  CredentialFieldValue,
  CredentialFormState,
  StatusFormState
} from "./index";

type OAuthReadableResult = {
  authUrl?: string;
  verificationUri?: string;
  userCode?: string;
  interval?: string;
  instructions?: string;
};

export type CredentialsTabViewModel = {
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
  oauthCallbackQueryByCredential: Record<number, string>;
  oauthResultByCredential: Record<number, string>;
  statusEditorCredentialId: number | null;
  statusForm: StatusFormState;
  credentialForm: CredentialFormState;
};

export type CredentialsTabActions = {
  setOauthStartQueryByCredential: Dispatch<SetStateAction<Record<number, string>>>;
  setOauthCallbackQueryByCredential: Dispatch<SetStateAction<Record<number, string>>>;
  setStatusEditorCredentialId: Dispatch<SetStateAction<number | null>>;
  setStatusForm: Dispatch<SetStateAction<StatusFormState>>;
  setCredentialForm: Dispatch<SetStateAction<CredentialFormState>>;
  onEditCredential: (row: CredentialQueryRow) => void;
  onCopyCredential: (row: CredentialQueryRow) => void;
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
  viewModel,
  actions,
  t
}: {
  viewModel: CredentialsTabViewModel;
  actions: CredentialsTabActions;
  t: TranslateFn;
}) {
  const {
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
    oauthCallbackQueryByCredential,
    oauthResultByCredential,
    statusEditorCredentialId,
    statusForm,
    credentialForm
  } = viewModel;
  const {
    setOauthStartQueryByCredential,
    setOauthCallbackQueryByCredential,
    setStatusEditorCredentialId,
    setStatusForm,
    setCredentialForm,
    onEditCredential,
    onCopyCredential,
    onRemoveCredential,
    onToggleCredentialEnabled,
    onSetCredentialHealth,
    onQueryUpstreamUsage,
    onUpsertStatus,
    onRunCredentialOAuthStart,
    onRunCredentialOAuthCallback,
    onUpsertCredential,
    onUpsertCredentialsBatch
  } = actions;

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
  const [singleQuickAddText, setSingleQuickAddText] = useState("");
  const [singleQuickAddError, setSingleQuickAddError] = useState("");
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
    setSingleQuickAddText("");
    setSingleQuickAddError("");
    setExpandedCooldownCredentialId(null);
    setSelectedCooldownKeysByCredential({});
  }, [channel, credentialSchema, supportsOAuth]);

  useEffect(() => {
    if (!supportsOAuth && subTab === "oauth") {
      setSubTab("single");
    }
  }, [subTab, supportsOAuth]);

  const singleQuickAddPlaceholder = useMemo(() => {
    if (channel === "claudecode") {
      return t("providers.singleQuick.placeholder.claudecode");
    }
    return t("providers.singleQuick.placeholder.default");
  }, [channel, t]);

  const runSingleQuickAdd = () => {
    const rawText = singleQuickAddText.trim();
    if (!rawText) {
      setSingleQuickAddError(t("providers.bulk.emptyImport"));
      return;
    }

    try {
      const looksLikeJson = rawText.startsWith("{") || rawText.startsWith("[");
      const mode: CredentialBulkMode = looksLikeJson
        ? "json"
        : channel === "claudecode"
          ? "claudecode_cookie"
          : credentialSchema.fields.some((field) => field.key === "api_key")
            ? "keys"
            : (() => {
                throw new Error(t("providers.singleQuick.keyUnsupported"));
              })();

      const entries = parseBulkCredentialText({
        channel,
        schema: credentialSchema,
        mode,
        rawText
      });
      if (entries.length === 0) {
        throw new Error(t("providers.bulk.emptyImport"));
      }
      if (entries.length > 1) {
        throw new Error(t("providers.singleQuick.singleOnly"));
      }
      setSingleQuickAddError("");
      onUpsertCredentialsBatch(entries);
      setSingleQuickAddText("");
    } catch (error) {
      setSingleQuickAddError(error instanceof Error ? error.message : String(error));
    }
  };

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
      kind === "start" ? oauthUi?.startDefaults?.[field] : oauthUi?.callbackDefaults?.[field];
    return (
      <div key={`${kind}-${field}`}>
        <Label>{translated === labelKey ? field : translated}</Label>
        <Input
          value={readQueryParam(rawQuery, field)}
          onChange={(value) =>
            kind === "start" ? setOAuthStartParam(field, value) : setOAuthCallbackParam(field, value)
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
              setSecretField(field.key, next === "true" ? true : next === "false" ? false : null)
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
      <CredentialCardsSection
        channel={channel}
        credentialRows={credentialRows}
        statusesByCredential={statusesByCredential}
        usageByCredential={usageByCredential}
        liveUsageRowsByCredential={liveUsageRowsByCredential}
        usageDisplayKindByCredential={usageDisplayKindByCredential}
        usageDisplayRowsByCredential={usageDisplayRowsByCredential}
        usageLoadingByCredential={usageLoadingByCredential}
        usageErrorByCredential={usageErrorByCredential}
        supportsUpstreamUsage={supportsUpstreamUsage}
        expandedCooldownCredentialId={expandedCooldownCredentialId}
        setExpandedCooldownCredentialId={setExpandedCooldownCredentialId}
        selectedCooldownKeysByCredential={selectedCooldownKeysByCredential}
        setSelectedCooldownKeysByCredential={setSelectedCooldownKeysByCredential}
        statusEditorCredentialId={statusEditorCredentialId}
        setStatusEditorCredentialId={setStatusEditorCredentialId}
        statusForm={statusForm}
        setStatusForm={setStatusForm}
        onEditCredential={onEditCredential}
        onCopyCredential={onCopyCredential}
        onRemoveCredential={onRemoveCredential}
        onToggleCredentialEnabled={onToggleCredentialEnabled}
        onSetCredentialHealth={onSetCredentialHealth}
        onQueryUpstreamUsage={onQueryUpstreamUsage}
        onUpsertStatus={onUpsertStatus}
        normalizeHealthKind={normalizeHealthKind}
        parseCooldowns={parseCooldowns}
        healthLabel={healthLabel}
        cooldownKey={cooldownKey}
        formatWindowLabel={formatWindowLabel}
        resolveUsageGroupLabel={resolveUsageGroupLabel}
        resolveLiveLimitLabel={resolveLiveLimitLabel}
        t={t}
      />

      <CredentialsSubTabs
        subTab={subTab}
        setSubTab={setSubTab}
        supportsOAuth={supportsOAuth}
        t={t}
      />

      {subTab === "single" ? (
        <div className="space-y-3">
          <CredentialSingleSection
            credentialForm={credentialForm}
            setCredentialForm={setCredentialForm}
            credentialSchema={credentialSchema}
            renderCredentialField={renderCredentialField}
            onUpsertCredential={onUpsertCredential}
            extraSectionBeforeOAuth={
              <div className="space-y-3 rounded-xl border border-border p-3">
                <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                  {t("providers.singleQuick.title")}
                </div>
                <div className="text-sm text-muted">{t("providers.singleQuick.hint")}</div>
                <TextArea
                  rows={4}
                  value={singleQuickAddText}
                  onChange={(value) => {
                    setSingleQuickAddText(value);
                    setSingleQuickAddError("");
                  }}
                  placeholder={singleQuickAddPlaceholder}
                />
                {singleQuickAddError ? (
                  <div className="text-sm text-red-500">{singleQuickAddError}</div>
                ) : null}
                <div className="flex flex-wrap gap-2">
                  <Button onClick={runSingleQuickAdd}>{t("providers.singleQuick.add")}</Button>
                  <Button
                    variant="neutral"
                    onClick={() => {
                      setSingleQuickAddText("");
                      setSingleQuickAddError("");
                    }}
                  >
                    {t("providers.bulk.clearInput")}
                  </Button>
                </div>
              </div>
            }
            t={t}
          />
        </div>
      ) : subTab === "oauth" ? (
        supportsOAuth ? (
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
                  onClick={() => onRunCredentialOAuthStart(undefined, button.mode, button.queryDefaults)}
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
        ) : (
          <div className="provider-card text-sm text-muted">{t("providers.oauth.unsupported")}</div>
        )
      ) : (
        <CredentialBulkSection
          bulkModes={bulkModes}
          bulkMode={bulkMode}
          setBulkMode={setBulkMode}
          bulkInputText={bulkInputText}
          setBulkInputText={setBulkInputText}
          bulkPlaceholder={bulkPlaceholder}
          bulkError={bulkError}
          setBulkError={setBulkError}
          runBulkImport={runBulkImport}
          runBulkExport={runBulkExport}
          openBulkImportFilePicker={openBulkImportFilePicker}
          runBulkExportFile={runBulkExportFile}
          bulkFileInputRef={bulkFileInputRef}
          onBulkImportFileChange={onBulkImportFileChange}
          bulkExportText={bulkExportText}
          t={t}
        />
      )}
    </div>
  );
}

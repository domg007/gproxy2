import { useCallback, useEffect, useMemo, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import {
  parseOptionalI64,
  parseRequiredI64
} from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type {
  CredentialQueryRow,
  CredentialStatusQueryRow,
  ProviderQueryRow
} from "../../lib/types";
import { Button, Card } from "../../components/ui";
import { ConfigTab } from "./providers/ConfigTab";
import { CredentialsTab } from "./providers/CredentialsTab";
import { ProviderList } from "./providers/ProviderList";
import {
  CHANNEL_SELECT_OPTIONS,
  type BulkCredentialImportEntry,
  type DispatchRuleDraft,
  type LiveUsageRow,
  type UsageDisplayKind,
  type UsageDisplayRow,
  type UsageSampleRow,
  type WorkspaceTab,
  buildUsageDisplayRows,
  buildUsageWindowSpecs,
  buildChannelSettingsJson,
  buildCredentialSecretJson,
  buildDispatchJson,
  createEmptyCredentialFormState,
  createDefaultDispatchRule,
  createEmptyProviderFormState,
  credentialFormFromRow,
  isCustomChannel,
  credentialSchemaForChannel,
  mergeQueryString,
  normalizeChannel,
  normalizeDispatchRules,
  parseLiveUsageRows,
  supportsOAuth,
  supportsUpstreamUsage,
  toProviderFormState,
  usagePayloadToText
} from "./providers";

function extractOAuthState(payload: unknown): string | undefined {
  let objectPayload: Record<string, unknown> | null = null;
  if (payload && typeof payload === "object" && !Array.isArray(payload)) {
    objectPayload = payload as Record<string, unknown>;
  } else if (typeof payload === "string") {
    const trimmed = payload.trim();
    if (trimmed) {
      try {
        const parsed = JSON.parse(trimmed);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          objectPayload = parsed as Record<string, unknown>;
        }
      } catch {
        return undefined;
      }
    }
  }
  if (!objectPayload) {
    return undefined;
  }
  const state = objectPayload.state;
  if (typeof state !== "string") {
    return undefined;
  }
  const trimmed = state.trim();
  return trimmed ? trimmed : undefined;
}

export function ProvidersModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [activeTab, setActiveTab] = useState<WorkspaceTab>("config");
  const [isCreatingProvider, setIsCreatingProvider] = useState(false);

  const [providerRows, setProviderRows] = useState<ProviderQueryRow[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<number | null>(null);
  const [providerForm, setProviderForm] = useState(createEmptyProviderFormState);

  const [credentialRows, setCredentialRows] = useState<CredentialQueryRow[]>([]);
  const [credentialForm, setCredentialForm] = useState(createEmptyCredentialFormState("custom"));

  const [statusRows, setStatusRows] = useState<CredentialStatusQueryRow[]>([]);
  const [statusEditorCredentialId, setStatusEditorCredentialId] = useState<number | null>(null);
  const [statusForm, setStatusForm] = useState({
    id: "",
    credentialId: "",
    healthKind: "healthy",
    healthJson: "",
    checkedAtUnixMs: "",
    lastError: ""
  });

  const [usageByCredential, setUsageByCredential] = useState<Record<number, string>>({});
  const [liveUsageRowsByCredential, setLiveUsageRowsByCredential] = useState<
    Record<number, LiveUsageRow[]>
  >({});
  const [usageDisplayKindByCredential, setUsageDisplayKindByCredential] = useState<
    Record<number, UsageDisplayKind>
  >({});
  const [usageDisplayRowsByCredential, setUsageDisplayRowsByCredential] = useState<
    Record<number, UsageDisplayRow[]>
  >({});
  const [usageLoadingByCredential, setUsageLoadingByCredential] = useState<Record<number, boolean>>(
    {}
  );
  const [usageErrorByCredential, setUsageErrorByCredential] = useState<Record<number, string>>({});
  const [oauthStartQueryByCredential, setOauthStartQueryByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthCallbackQueryByCredential, setOauthCallbackQueryByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthResultByCredential, setOauthResultByCredential] = useState<Record<number, string>>(
    {}
  );

  const selectedProvider = useMemo(
    () => providerRows.find((row) => row.id === selectedProviderId) ?? null,
    [providerRows, selectedProviderId]
  );

  const providerRouteKey = selectedProvider ? encodeURIComponent(selectedProvider.channel) : "";
  const currentCredentialSchema = credentialSchemaForChannel(
    selectedProvider?.channel ?? providerForm.channel
  );
  const providerSupportsOAuth = selectedProvider
    ? supportsOAuth(selectedProvider.channel)
    : false;
  const providerSupportsUpstreamUsage = selectedProvider
    ? supportsUpstreamUsage(selectedProvider.channel)
    : false;
  const showWorkspace = isCreatingProvider || selectedProvider !== null;

  const providerFormChannel = normalizeChannel(providerForm.channel);
  const showCodexOAuthIssuer = providerFormChannel === "codex";
  const showOAuthTriplet =
    providerFormChannel === "geminicli" || providerFormChannel === "antigravity";
  const showVertexOAuthToken = providerFormChannel === "vertex";
  const showClaudeCodeSettings = providerFormChannel === "claudecode";
  const showCustomMaskTable = isCustomChannel(providerFormChannel);

  const channelOptions = useMemo(() => {
    const options = [...CHANNEL_SELECT_OPTIONS];
    const current = providerForm.channel.trim();
    if (current && !options.some((item) => item.value === current)) {
      options.push({ value: current, label: `${current} (custom)` });
    }
    return options;
  }, [providerForm.channel]);

  const credentialIdSet = useMemo(
    () => new Set(credentialRows.map((row) => row.id)),
    [credentialRows]
  );

  const scopedStatusRows = useMemo(
    () => statusRows.filter((row) => credentialIdSet.has(row.credential_id)),
    [credentialIdSet, statusRows]
  );

  const statusesByCredential = useMemo(() => {
    const map = new Map<number, CredentialStatusQueryRow[]>();
    for (const row of scopedStatusRows) {
      const list = map.get(row.credential_id);
      if (list) {
        list.push(row);
      } else {
        map.set(row.credential_id, [row]);
      }
    }
    return map;
  }, [scopedStatusRows]);

  useEffect(() => {
    if (!showWorkspace && activeTab !== "config") {
      setActiveTab("config");
    }
  }, [activeTab, showWorkspace]);

  const loadProviders = useCallback(async () => {
    try {
      const data = await apiRequest<ProviderQueryRow[]>("/admin/providers/query", {
        apiKey,
        method: "POST",
        body: {
          channel: scopeAll<string>(),
          name: scopeAll<string>(),
          enabled: scopeAll<boolean>(),
          limit: 300
        }
      });
      const sorted = [...data].sort((a, b) => a.id - b.id);
      setProviderRows(sorted);
      setSelectedProviderId((prev) => {
        if (prev !== null && sorted.some((row) => row.id === prev)) {
          return prev;
        }
        return null;
      });
    } catch (error) {
      notify("error", formatError(error));
    }
  }, [apiKey, notify]);

  const loadProviderScopedData = useCallback(
    async (provider: ProviderQueryRow | null) => {
      if (!provider) {
        setCredentialRows([]);
        setStatusRows([]);
        return;
      }
      try {
        const [credentials, statuses] = await Promise.all([
          apiRequest<CredentialQueryRow[]>("/admin/credentials/query", {
            apiKey,
            method: "POST",
            body: {
              provider_id: scopeEq(provider.id),
              kind: scopeAll<string>(),
              enabled: scopeAll<boolean>(),
              limit: 500
            }
          }),
          apiRequest<CredentialStatusQueryRow[]>("/admin/credential-statuses/query", {
            apiKey,
            method: "POST",
            body: {
              id: scopeAll<number>(),
              credential_id: scopeAll<number>(),
              channel: scopeEq(provider.channel),
              health_kind: scopeAll<string>(),
              limit: 500
            }
          })
        ]);
        setCredentialRows(credentials);
        setStatusRows(statuses);
      } catch (error) {
        notify("error", formatError(error));
      }
    },
    [apiKey, notify]
  );

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    void loadProviderScopedData(selectedProvider);
    setUsageByCredential({});
    setLiveUsageRowsByCredential({});
    setUsageDisplayKindByCredential({});
    setUsageDisplayRowsByCredential({});
    setUsageLoadingByCredential({});
    setUsageErrorByCredential({});
    setOauthStartQueryByCredential({});
    setOauthCallbackQueryByCredential({});
    setOauthResultByCredential({});
    setStatusEditorCredentialId(null);
    setCredentialForm(
      createEmptyCredentialFormState(selectedProvider?.channel ?? providerForm.channel)
    );
  }, [loadProviderScopedData, selectedProvider]);

  const beginCreateProvider = () => {
    setProviderForm(createEmptyProviderFormState());
    setSelectedProviderId(null);
    setIsCreatingProvider(true);
    setActiveTab("config");
  };

  const editProvider = (row: ProviderQueryRow) => {
    setProviderForm(toProviderFormState(row));
    setSelectedProviderId(row.id);
    setIsCreatingProvider(false);
    setActiveTab("config");
  };

  const selectProvider = (row: ProviderQueryRow) => {
    setProviderForm(toProviderFormState(row));
    setSelectedProviderId(row.id);
    setIsCreatingProvider(false);
    setActiveTab("config");
  };

  const upsertProvider = async () => {
    try {
      const savedId = parseRequiredI64(providerForm.id, "id");
      const rules = normalizeDispatchRules(providerForm.dispatchRules);
      const dispatchJson = buildDispatchJson(rules);
      await apiRequest("/admin/providers/upsert", {
        apiKey,
        method: "POST",
        body: {
          id: savedId,
          name: providerForm.name.trim(),
          channel: providerForm.channel.trim(),
          settings_json: JSON.stringify(
            buildChannelSettingsJson(providerForm.channel, providerForm.settings)
          ),
          dispatch_json: JSON.stringify(dispatchJson),
          enabled: providerForm.enabled
        }
      });
      notify("success", t("providers.saved"));
      setIsCreatingProvider(false);
      setSelectedProviderId(savedId);
      await loadProviders();
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const removeProvider = async (id: number) => {
    try {
      await apiRequest("/admin/providers/delete", {
        apiKey,
        method: "POST",
        body: { id }
      });
      notify("success", t("providers.deleted", { id }));
      if (selectedProviderId === id) {
        setSelectedProviderId(null);
      }
      await loadProviders();
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const toggleProviderEnabled = async (row: ProviderQueryRow) => {
    const nextEnabled = !row.enabled;
    setProviderRows((prev) =>
      prev.map((item) => (item.id === row.id ? { ...item, enabled: nextEnabled } : item))
    );
    if (selectedProviderId === row.id) {
      setProviderForm((prev) => ({ ...prev, enabled: nextEnabled }));
    }

    try {
      await apiRequest("/admin/providers/upsert", {
        apiKey,
        method: "POST",
        body: {
          id: row.id,
          name: row.name,
          channel: row.channel,
          settings_json: JSON.stringify(row.settings_json),
          dispatch_json: JSON.stringify(row.dispatch_json),
          enabled: nextEnabled
        }
      });
      notify(
        "success",
        t("providers.enabledChanged", {
          id: row.id,
          state: nextEnabled ? t("common.enabled") : t("common.disabled")
        })
      );
      window.setTimeout(() => {
        void loadProviders();
      }, 250);
    } catch (error) {
      setProviderRows((prev) =>
        prev.map((item) => (item.id === row.id ? { ...item, enabled: row.enabled } : item))
      );
      if (selectedProviderId === row.id) {
        setProviderForm((prev) => ({ ...prev, enabled: row.enabled }));
      }
      notify("error", formatError(error));
    }
  };

  const addDispatchRule = () => {
    setProviderForm((prev) => ({
      ...prev,
      dispatchRules: [...prev.dispatchRules, createDefaultDispatchRule()]
    }));
  };

  const updateDispatchRule = (
    id: string,
    patch: Partial<Omit<DispatchRuleDraft, "id">>
  ) => {
    setProviderForm((prev) => ({
      ...prev,
      dispatchRules: prev.dispatchRules.map((rule) =>
        rule.id === id ? { ...rule, ...patch } : rule
      )
    }));
  };

  const removeDispatchRule = (id: string) => {
    setProviderForm((prev) => {
      const next = prev.dispatchRules.filter((rule) => rule.id !== id);
      return {
        ...prev,
        dispatchRules: next.length ? next : [createDefaultDispatchRule()]
      };
    });
  };

  const upsertCredential = async () => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    try {
      const id = parseRequiredI64(credentialForm.id, "id");
      const secretJson = buildCredentialSecretJson(
        selectedProvider.channel,
        credentialForm.secretValues
      );
      await apiRequest("/admin/credentials/upsert", {
        apiKey,
        method: "POST",
        body: {
          id,
          provider_id: selectedProvider.id,
          name: credentialForm.name.trim() || null,
          kind: currentCredentialSchema.kind,
          settings_json: credentialForm.settingsPayload
            ? JSON.stringify(credentialForm.settingsPayload)
            : null,
          secret_json: secretJson,
          enabled: credentialForm.enabled
        }
      });
      const now = new Date().toISOString();
      setCredentialRows((prev) => {
        const next = prev.slice();
        const index = next.findIndex((row) => row.id === id);
        const row: CredentialQueryRow = {
          id,
          provider_id: selectedProvider.id,
          name: credentialForm.name.trim() || null,
          kind: currentCredentialSchema.kind,
          settings_json: credentialForm.settingsPayload ?? null,
          secret_json: JSON.parse(secretJson) as Record<string, unknown>,
          enabled: credentialForm.enabled,
          created_at: index >= 0 ? next[index].created_at : now,
          updated_at: now
        };
        if (index >= 0) {
          next[index] = row;
        } else {
          next.unshift(row);
        }
        return next;
      });
      notify("success", t("providers.credentials.saved"));
      window.setTimeout(() => {
        void loadProviderScopedData(selectedProvider);
      }, 250);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const upsertCredentialsBatch = async (entries: BulkCredentialImportEntry[]) => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    if (entries.length === 0) {
      notify("error", t("providers.bulk.emptyImport"));
      return;
    }

    try {
      const takenIds = new Set(credentialRows.map((row) => row.id));
      const usedInBatch = new Set<number>();
      let nextId = credentialRows.reduce((max, row) => Math.max(max, row.id), 0) + 1;

      for (const entry of entries) {
        const candidateId = entry.id;
        let id: number;
        if (typeof candidateId === "number") {
          id = candidateId;
        } else {
          while (takenIds.has(nextId) || usedInBatch.has(nextId)) {
            nextId += 1;
          }
          id = nextId;
          usedInBatch.add(id);
          nextId += 1;
        }

        const secretJson = buildCredentialSecretJson(
          selectedProvider.channel,
          entry.secretValues
        );

        await apiRequest("/admin/credentials/upsert", {
          apiKey,
          method: "POST",
          body: {
            id,
            provider_id: selectedProvider.id,
            name: entry.name?.trim() || null,
            kind: currentCredentialSchema.kind,
            settings_json: entry.settingsPayload
              ? JSON.stringify(entry.settingsPayload)
              : null,
            secret_json: secretJson,
            enabled: entry.enabled ?? true
          }
        });
      }

      notify("success", t("providers.bulk.imported", { count: entries.length }));
      await loadProviderScopedData(selectedProvider);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const removeCredential = async (id: number) => {
    if (!selectedProvider) {
      return;
    }
    try {
      await apiRequest("/admin/credentials/delete", {
        apiKey,
        method: "POST",
        body: { id }
      });
      setCredentialRows((prev) => prev.filter((row) => row.id !== id));
      setStatusRows((prev) => prev.filter((row) => row.credential_id !== id));
      setUsageByCredential((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setLiveUsageRowsByCredential((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setUsageDisplayKindByCredential((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setUsageDisplayRowsByCredential((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setUsageLoadingByCredential((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setUsageErrorByCredential((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      notify("success", t("providers.credentials.deleted", { id }));
      window.setTimeout(() => {
        void loadProviderScopedData(selectedProvider);
      }, 250);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const toggleCredentialEnabled = async (row: CredentialQueryRow) => {
    if (!selectedProvider) {
      return;
    }
    const nextEnabled = !row.enabled;
    setCredentialRows((prev) =>
      prev.map((item) => (item.id === row.id ? { ...item, enabled: nextEnabled } : item))
    );
    try {
      await apiRequest("/admin/credentials/upsert", {
        apiKey,
        method: "POST",
        body: {
          id: row.id,
          provider_id: row.provider_id,
          name: row.name,
          kind: row.kind,
          settings_json: row.settings_json ? JSON.stringify(row.settings_json) : null,
          secret_json: JSON.stringify(row.secret_json),
          enabled: nextEnabled
        }
      });
      notify(
        "success",
        t("providers.credentials.enabledChanged", {
          id: row.id,
          state: nextEnabled ? t("common.enabled") : t("common.disabled")
        })
      );
      window.setTimeout(() => {
        void loadProviderScopedData(selectedProvider);
      }, 250);
    } catch (error) {
      setCredentialRows((prev) =>
        prev.map((item) => (item.id === row.id ? { ...item, enabled: row.enabled } : item))
      );
      notify("error", formatError(error));
    }
  };

  const upsertCredentialHealthState = async ({
    credentialId,
    statusId,
    healthKind,
    healthJson,
    lastError
  }: {
    credentialId: number;
    statusId?: number;
    healthKind: "healthy" | "partial" | "dead";
    healthJson: Record<string, unknown> | null;
    lastError?: string | null;
  }) => {
    if (!selectedProvider) {
      return;
    }
    const checkedAtMs = Date.now();
    try {
      await apiRequest("/admin/credential-statuses/upsert", {
        apiKey,
        method: "POST",
        body: {
          id: statusId,
          credential_id: credentialId,
          channel: selectedProvider.channel,
          health_kind: healthKind,
          health_json: healthJson ? JSON.stringify(healthJson) : null,
          checked_at_unix_ms: checkedAtMs,
          last_error: lastError ?? null
        }
      });
      const nowIso = new Date(checkedAtMs).toISOString();
      setStatusRows((prev) => {
        const next = prev.slice();
        const index = next.findIndex(
          (row) =>
            row.credential_id === credentialId && row.channel === selectedProvider.channel
        );
        const base = index >= 0 ? next[index] : undefined;
        const nextRow: CredentialStatusQueryRow = {
          id: statusId ?? base?.id ?? -credentialId,
          credential_id: credentialId,
          channel: selectedProvider.channel,
          health_kind: healthKind,
          health_json: healthJson,
          checked_at: nowIso,
          last_error: lastError ?? null,
          updated_at: nowIso
        };
        if (index >= 0) {
          next[index] = nextRow;
        } else {
          next.push(nextRow);
        }
        return next;
      });
      notify("success", t("providers.status.saved"));
      window.setTimeout(() => {
        void loadProviderScopedData(selectedProvider);
      }, 250);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const editCredential = (row: CredentialQueryRow) => {
    setCredentialForm(
      credentialFormFromRow(selectedProvider?.channel ?? providerForm.channel, row)
    );
    setActiveTab("credentials");
  };

  const queryUpstreamUsage = async (credentialId: number) => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    setUsageLoadingByCredential((prev) => ({ ...prev, [credentialId]: true }));
    setUsageErrorByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
    try {
      const path = `/${providerRouteKey}/v1/usage?credential_id=${encodeURIComponent(String(credentialId))}`;
      const payload = await apiRequest<unknown>(path, {
        apiKey,
        method: "GET"
      });
      const nowMs = Date.now();
      const liveRows = parseLiveUsageRows(selectedProvider.channel, payload);
      const specs = buildUsageWindowSpecs(selectedProvider.channel, payload, liveRows, nowMs);
      let usageRows: UsageSampleRow[] = [];

      if (specs.length > 0) {
        const minFromUnixMs = specs.reduce(
          (min, item) => Math.min(min, item.fromUnixMs),
          Number.MAX_SAFE_INTEGER
        );
        const maxToUnixMs = specs.reduce((max, item) => Math.max(max, item.toUnixMs), 0);
        const rows = await apiRequest<
          Array<
            UsageSampleRow & {
              credential_id: number | null;
            }
          >
        >("/admin/usages/query", {
          apiKey,
          method: "POST",
          body: {
            channel: scopeEq(selectedProvider.channel),
            model: scopeAll<string>(),
            user_id: scopeAll<number>(),
            user_key_id: scopeAll<number>(),
            from_unix_ms: minFromUnixMs,
            to_unix_ms: maxToUnixMs,
            limit: 0
          }
        });
        usageRows = rows.filter((row) => row.credential_id === credentialId);
      }

      const usageDisplay = buildUsageDisplayRows(
        selectedProvider.channel,
        liveRows,
        specs,
        usageRows
      );

      setUsageByCredential((prev) => ({
        ...prev,
        [credentialId]: usagePayloadToText(payload)
      }));
      setLiveUsageRowsByCredential((prev) => ({
        ...prev,
        [credentialId]: liveRows
      }));
      setUsageDisplayKindByCredential((prev) => ({
        ...prev,
        [credentialId]: usageDisplay.kind
      }));
      setUsageDisplayRowsByCredential((prev) => ({
        ...prev,
        [credentialId]: usageDisplay.rows
      }));
      notify("success", t("providers.usage.fetched", { id: credentialId }));
    } catch (error) {
      setUsageErrorByCredential((prev) => ({
        ...prev,
        [credentialId]: formatError(error)
      }));
      notify("error", formatError(error));
    } finally {
      setUsageLoadingByCredential((prev) => ({ ...prev, [credentialId]: false }));
    }
  };

  const upsertStatus = async () => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    try {
      await apiRequest("/admin/credential-statuses/upsert", {
        apiKey,
        method: "POST",
        body: {
          id: parseOptionalI64(statusForm.id),
          credential_id: parseRequiredI64(statusForm.credentialId, "credential_id"),
          channel: selectedProvider.channel,
          health_kind: statusForm.healthKind.trim(),
          health_json: statusForm.healthJson.trim() || null,
          checked_at_unix_ms: parseOptionalI64(statusForm.checkedAtUnixMs),
          last_error: statusForm.lastError.trim() || null
        }
      });
      notify("success", t("providers.status.saved"));
      await loadProviderScopedData(selectedProvider);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const runCredentialOAuthStart = async (
    credentialId?: number,
    mode?: string,
    queryDefaults?: Record<string, string | null | undefined>
  ) => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    try {
      const key = credentialId ?? 0;
      const query = mergeQueryString(oauthStartQueryByCredential[key] ?? "", {
        ...(queryDefaults ?? {}),
        credential_id: credentialId === undefined ? undefined : String(credentialId),
        mode
      });
      const payload = await apiRequest<unknown>(`/${providerRouteKey}/v1/oauth${query}`, {
        apiKey,
        method: "GET"
      });
      const oauthState = extractOAuthState(payload);
      if (oauthState) {
        setOauthCallbackQueryByCredential((prev) => ({
          ...prev,
          [key]: mergeQueryString(prev[key] ?? "", { state: oauthState })
        }));
      }
      setOauthResultByCredential((prev) => ({
        ...prev,
        [key]: usagePayloadToText(payload)
      }));
      notify("success", t("providers.oauth.startDone"));
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const runCredentialOAuthCallback = async (
    credentialId?: number,
    mode?: string,
    queryDefaults?: Record<string, string | null | undefined>
  ) => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    try {
      const key = credentialId ?? 0;
      const query = mergeQueryString(oauthCallbackQueryByCredential[key] ?? "", {
        ...(queryDefaults ?? {}),
        credential_id: credentialId === undefined ? undefined : String(credentialId),
        mode
      });
      const payload = await apiRequest<unknown>(`/${providerRouteKey}/v1/oauth/callback${query}`, {
        apiKey,
        method: "GET"
      });
      setOauthResultByCredential((prev) => ({
        ...prev,
        [key]: usagePayloadToText(payload)
      }));
      notify("success", t("providers.oauth.callbackDone"));
      await loadProviderScopedData(selectedProvider);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  return (
    <div className="space-y-4">
      <Card
        title={t("providers.title")}
        subtitle={t("providers.subtitle")}
        action={
          <div className="flex flex-wrap gap-2">
            <Button variant="neutral" onClick={() => void loadProviders()}>
              {t("providers.refresh")}
            </Button>
            <Button onClick={beginCreateProvider}>{t("providers.create")}</Button>
          </div>
        }
      >
        <div className="space-y-4">
          <ProviderList
            providerRows={providerRows}
            selectedProviderId={selectedProviderId}
            onSelectProvider={selectProvider}
            onToggleEnabled={(row) => void toggleProviderEnabled(row)}
            onEdit={editProvider}
            onDelete={(id) => void removeProvider(id)}
            t={t}
          />

          {!showWorkspace ? (
            <div className="provider-card text-sm text-muted">{t("providers.selectHint")}</div>
          ) : (
            <div className="space-y-4">
              <div className="flex flex-wrap gap-2">
                {([
                  { id: "config", label: t("providers.tab.config"), enabled: true },
                  {
                    id: "credentials",
                    label: t("providers.tab.credentials"),
                    enabled: !!selectedProvider
                  }
                ] as Array<{ id: WorkspaceTab; label: string; enabled: boolean }>).map((tab) => (
                  <button
                    key={tab.id}
                    type="button"
                    className={`workspace-tab ${activeTab === tab.id ? "workspace-tab-active" : ""}`}
                    disabled={!tab.enabled}
                    onClick={() => setActiveTab(tab.id)}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              {activeTab === "config" ? (
                <ConfigTab
                  providerForm={providerForm}
                  setProviderForm={setProviderForm}
                  channelOptions={channelOptions}
                  showCodexOAuthIssuer={showCodexOAuthIssuer}
                  showOAuthTriplet={showOAuthTriplet}
                  showVertexOAuthToken={showVertexOAuthToken}
                  showClaudeCodeSettings={showClaudeCodeSettings}
                  showCustomMaskTable={showCustomMaskTable}
                  addDispatchRule={addDispatchRule}
                  updateDispatchRule={updateDispatchRule}
                  removeDispatchRule={removeDispatchRule}
                  isCreatingProvider={isCreatingProvider}
                  onCancelCreate={() => setIsCreatingProvider(false)}
                  onSave={() => void upsertProvider()}
                  t={t}
                />
              ) : null}

              {activeTab === "credentials" ? (
                <CredentialsTab
                  selectedProvider={selectedProvider}
                  credentialSchema={currentCredentialSchema}
                  supportsUpstreamUsage={providerSupportsUpstreamUsage}
                  supportsOAuth={providerSupportsOAuth}
                  credentialRows={credentialRows}
                  statusesByCredential={statusesByCredential}
                  usageByCredential={usageByCredential}
                  liveUsageRowsByCredential={liveUsageRowsByCredential}
                  usageDisplayKindByCredential={usageDisplayKindByCredential}
                  usageDisplayRowsByCredential={usageDisplayRowsByCredential}
                  usageLoadingByCredential={usageLoadingByCredential}
                  usageErrorByCredential={usageErrorByCredential}
                  oauthStartQueryByCredential={oauthStartQueryByCredential}
                  setOauthStartQueryByCredential={setOauthStartQueryByCredential}
                  oauthCallbackQueryByCredential={oauthCallbackQueryByCredential}
                  setOauthCallbackQueryByCredential={setOauthCallbackQueryByCredential}
                  oauthResultByCredential={oauthResultByCredential}
                  statusEditorCredentialId={statusEditorCredentialId}
                  setStatusEditorCredentialId={setStatusEditorCredentialId}
                  statusForm={statusForm}
                  setStatusForm={setStatusForm}
                  credentialForm={credentialForm}
                  setCredentialForm={setCredentialForm}
                  onEditCredential={editCredential}
                  onRemoveCredential={(id) => void removeCredential(id)}
                  onToggleCredentialEnabled={(row) => void toggleCredentialEnabled(row)}
                  onSetCredentialHealth={(payload) => void upsertCredentialHealthState(payload)}
                  onQueryUpstreamUsage={(id) => void queryUpstreamUsage(id)}
                  onUpsertStatus={() => void upsertStatus()}
                  onRunCredentialOAuthStart={(id, mode, queryDefaults) =>
                    void runCredentialOAuthStart(id, mode, queryDefaults)
                  }
                  onRunCredentialOAuthCallback={(id, mode, queryDefaults) =>
                    void runCredentialOAuthCallback(id, mode, queryDefaults)
                  }
                  onUpsertCredential={() => void upsertCredential()}
                  onUpsertCredentialsBatch={(entries) => void upsertCredentialsBatch(entries)}
                  t={t}
                />
              ) : null}
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}

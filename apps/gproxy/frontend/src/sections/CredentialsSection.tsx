import { useCallback, useEffect, useMemo, useState } from "react";

import { request, formatApiError, safeParseJson } from "../lib/api";
import type { CredentialRow, ProviderDetail } from "../lib/types";
import {
  buildCredentialSecret,
  credentialFieldMap,
  extractCredentialFields,
  kindFromConfig,
  type FieldSpec
} from "../lib/provider_schema";
import { Badge, Button, Card, FieldLabel, TextArea, TextInput } from "../components/ui";
import { useI18n } from "../i18n";

type Props = {
  adminKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
};

function looksLikeTaggedCredential(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== "object") {
    return false;
  }
  return Object.keys(value).length === 1;
}

export function CredentialsSection({ adminKey, notify }: Props) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<ProviderDetail[]>([]);
  const [selectedProvider, setSelectedProvider] = useState("");
  const [rows, setRows] = useState<CredentialRow[]>([]);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [displayName, setDisplayName] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [fields, setFields] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [importKeys, setImportKeys] = useState("");
  const [importFiles, setImportFiles] = useState<File[]>([]);

  const provider = useMemo(
    () => providers.find((item) => item.name === selectedProvider) ?? null,
    [providers, selectedProvider]
  );
  const providerKind = provider ? kindFromConfig(provider.config_json) : null;
  const fieldSpecs: FieldSpec[] = providerKind ? credentialFieldMap[providerKind] : [];
  const lineImportFieldKey = providerKind === "claudecode" ? "session_key" : "api_key";

  const loadProviders = useCallback(async () => {
    try {
      const list = await request<{ providers: Array<{ name: string }> }>("/admin/providers", { adminKey });
      const details = await Promise.all(
        (list.providers ?? []).map((item) => request<ProviderDetail>(`/admin/providers/${item.name}`, { adminKey }))
      );
      setProviders(details);
      if (!selectedProvider && details.length > 0) {
        setSelectedProvider(details[0].name);
      }
    } catch (error) {
      notify("error", formatApiError(error));
    }
  }, [adminKey, notify, selectedProvider]);

  const loadCredentials = useCallback(
    async (providerName: string) => {
      if (!providerName) {
        return;
      }
      setLoading(true);
      try {
        const data = await request<{ credentials: CredentialRow[] }>(
          `/admin/providers/${providerName}/credentials`,
          { adminKey }
        );
        setRows(data.credentials ?? []);
      } catch (error) {
        notify("error", formatApiError(error));
      } finally {
        setLoading(false);
      }
    },
    [adminKey, notify]
  );

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    if (selectedProvider) {
      void loadCredentials(selectedProvider);
      setEditingId(null);
      setDisplayName("");
      setFields({});
      setEnabled(true);
    }
  }, [selectedProvider, loadCredentials]);

  const submit = async () => {
    if (!provider || !providerKind) {
      notify("error", t("errors.missing_provider"));
      return;
    }
    try {
      const secretJson = buildCredentialSecret(providerKind, fields);
      if (editingId) {
        await request(`/admin/credentials/${editingId}`, {
          method: "PUT",
          adminKey,
          body: {
            name: displayName.trim() || null,
            secret_json: secretJson
          }
        });
        notify("success", t("credentials.update_ok"));
      } else {
        await request(`/admin/providers/${provider.name}/credentials`, {
          method: "POST",
          adminKey,
          body: {
            name: displayName.trim() || null,
            settings_json: {},
            secret_json: secretJson,
            enabled
          }
        });
        notify("success", t("credentials.create_ok"));
      }
      setEditingId(null);
      setDisplayName("");
      setFields({});
      setEnabled(true);
      await loadCredentials(provider.name);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const editRow = (row: CredentialRow) => {
    if (!providerKind) {
      return;
    }
    setEditingId(row.id);
    setDisplayName(row.name ?? "");
    setEnabled(row.enabled);
    setFields(extractCredentialFields(providerKind, row.secret_json));
  };

  const removeRow = async (row: CredentialRow) => {
    if (!confirm(`${t("common.delete")}? #${row.id}`)) {
      return;
    }
    try {
      await request(`/admin/credentials/${row.id}`, { method: "DELETE", adminKey });
      notify("success", t("credentials.delete_ok"));
      await loadCredentials(selectedProvider);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const setRowEnabled = async (row: CredentialRow, nextEnabled: boolean) => {
    try {
      await request(`/admin/credentials/${row.id}/enabled`, {
        method: "PUT",
        adminKey,
        body: { enabled: nextEnabled }
      });
      notify("success", t("credentials.toggle_ok"));
      await loadCredentials(selectedProvider);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const runImport = async () => {
    if (!provider || !providerKind) {
      notify("error", t("errors.missing_provider"));
      return;
    }

    try {
      const payloads: Array<Record<string, unknown>> = [];

      if (importKeys.trim()) {
        const lines = importKeys
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter(Boolean);
        for (const line of lines) {
          payloads.push(buildCredentialSecret(providerKind, { [lineImportFieldKey]: line }));
        }
      }

      for (const file of importFiles) {
        const text = await file.text();
        const parsed = safeParseJson(text);
        if (!parsed || typeof parsed !== "object") {
          continue;
        }

        if (looksLikeTaggedCredential(parsed)) {
          payloads.push(parsed as Record<string, unknown>);
          continue;
        }

        const simpleFields: Record<string, string> = {};
        for (const spec of credentialFieldMap[providerKind]) {
          const value = (parsed as Record<string, unknown>)[spec.key];
          if (value !== undefined && value !== null) {
            simpleFields[spec.key] = String(value);
          }
        }
        payloads.push(buildCredentialSecret(providerKind, simpleFields));
      }

      if (payloads.length === 0) {
        notify("info", t("common.empty"));
        return;
      }

      for (const secret_json of payloads) {
        await request(`/admin/providers/${provider.name}/credentials`, {
          method: "POST",
          adminKey,
          body: {
            name: null,
            settings_json: {},
            secret_json,
            enabled: true
          }
        });
      }

      setImportKeys("");
      setImportFiles([]);
      notify("success", t("credentials.import_ok"));
      await loadCredentials(provider.name);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const fieldLabel = (key: string) => t(`credentials.${key}`);

  return (
    <div className="space-y-5">
      <Card
        title={t("credentials.title")}
        subtitle={t("credentials.subtitle")}
        action={
          <Button variant="neutral" onClick={() => void loadCredentials(selectedProvider)} disabled={!selectedProvider || loading}>
            {t("common.refresh")}
          </Button>
        }
      >
        <div className="grid gap-4 md:grid-cols-2">
          <div>
            <FieldLabel>{t("credentials.select_provider")}</FieldLabel>
            <select
              className="mt-2 select"
              value={selectedProvider}
              onChange={(event) => setSelectedProvider(event.target.value)}
            >
              {providers.map((item) => (
                <option key={item.name} value={item.name}>
                  {item.name}
                </option>
              ))}
            </select>
          </div>
          <div className="flex items-end gap-3">
            <input id="credential-enabled" type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />
            <label htmlFor="credential-enabled" className="text-sm text-slate-700">{t("common.enabled")}</label>
          </div>
          <div className="md:col-span-2">
            <FieldLabel>{t("credentials.display_name")}</FieldLabel>
            <div className="mt-2">
              <TextInput value={displayName} onChange={setDisplayName} />
            </div>
          </div>

          {fieldSpecs.map((spec) => {
            const value = fields[spec.key] ?? "";
            const isWide = spec.type === "textarea";
            return (
              <div key={spec.key} className={isWide ? "md:col-span-2" : ""}>
                <FieldLabel>{fieldLabel(spec.key)}</FieldLabel>
                <div className="mt-2">
                  {spec.type === "textarea" ? (
                    <TextArea value={value} onChange={(next) => setFields((prev) => ({ ...prev, [spec.key]: next }))} rows={4} />
                  ) : (
                    <TextInput
                      value={value}
                      type={spec.type === "number" ? "number" : spec.type === "password" ? "password" : "text"}
                      onChange={(next) => setFields((prev) => ({ ...prev, [spec.key]: next }))}
                    />
                  )}
                </div>
              </div>
            );
          })}
        </div>

        <div className="mt-4 flex flex-wrap gap-2">
          <Button onClick={() => void submit()}>
            {editingId ? t("credentials.update_credential") : t("credentials.new_credential")}
          </Button>
          {editingId ? (
            <Button
              variant="neutral"
              onClick={() => {
                setEditingId(null);
                setDisplayName("");
                setFields({});
                setEnabled(true);
              }}
            >
              {t("common.cancel")}
            </Button>
          ) : null}
        </div>
      </Card>

      <Card title={t("credentials.import_hint")} subtitle={t("credentials.import_files")}>
        <div className="grid gap-4 md:grid-cols-2">
          <div>
            <FieldLabel>
              {providerKind === "claudecode"
                ? t("credentials.import_session_keys")
                : t("credentials.import_keys")}
            </FieldLabel>
            <div className="mt-2">
              <TextArea
                value={importKeys}
                onChange={setImportKeys}
                rows={6}
                placeholder={
                  providerKind === "claudecode"
                    ? t("credentials.import_session_keys_placeholder")
                    : t("credentials.import_keys_placeholder")
                }
              />
            </div>
          </div>
          <div>
            <FieldLabel>{t("credentials.import_files")}</FieldLabel>
            <div className="mt-2 space-y-2">
              <input
                type="file"
                multiple
                accept="application/json"
                onChange={(event) => setImportFiles(Array.from(event.target.files ?? []))}
              />
              <div className="text-xs text-slate-500">{importFiles.length} file(s)</div>
            </div>
          </div>
        </div>
        <div className="mt-4">
          <Button onClick={() => void runImport()}>{t("credentials.import_run")}</Button>
        </div>
      </Card>

      <Card title={t("credentials.list_title")} subtitle={selectedProvider ? `/admin/providers/${selectedProvider}/credentials` : ""}>
        {loading ? (
          <div className="text-sm text-slate-500">{t("common.loading")}</div>
        ) : rows.length === 0 ? (
          <div className="text-sm text-slate-500">{t("common.empty")}</div>
        ) : (
          <div className="space-y-3">
            {rows.map((row) => (
              <div key={row.id} className="rounded-2xl border border-slate-200 bg-white/70 p-4">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <div className="text-sm font-semibold text-slate-900">#{row.id} {row.name ?? ""}</div>
                  </div>
                  <Badge active={row.enabled}>{row.enabled ? t("common.enabled") : t("common.disabled")}</Badge>
                </div>
                <div className="mt-3 grid gap-2 sm:grid-cols-2">
                  <Button variant="neutral" onClick={() => editRow(row)}>{t("common.edit")}</Button>
                  <Button variant="neutral" onClick={() => void setRowEnabled(row, !row.enabled)}>
                    {row.enabled ? t("common.disabled") : t("common.enabled")}
                  </Button>
                  <Button variant="danger" onClick={() => void removeRow(row)}>{t("common.delete")}</Button>
                </div>
                <div className="mt-3 text-xs text-slate-500">
                  {Object.keys(row.secret_json).join(", ")}
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}

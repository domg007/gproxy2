import { useCallback, useEffect, useMemo, useState } from "react";

import { request, formatApiError } from "../lib/api";
import type { CredentialListRow, ProviderDetail, UsageResponse } from "../lib/types";
import { beforeHoursRfc3339, nowRfc3339 } from "../lib/format";
import { Button, Card, FieldLabel, TextInput } from "../components/ui";
import { useI18n } from "../i18n";

type Props = {
  adminKey: string;
  providers: ProviderDetail[];
  notify: (kind: "success" | "error" | "info", message: string) => void;
};

type QueryMode = "provider" | "provider_model" | "credential" | "credential_model";

export function UsageSection({ adminKey, providers, notify }: Props) {
  const { t } = useI18n();
  const [mode, setMode] = useState<QueryMode>("provider");
  const [provider, setProvider] = useState("");
  const [credentialId, setCredentialId] = useState("");
  const [model, setModel] = useState("");
  const [from, setFrom] = useState(beforeHoursRfc3339(24));
  const [to, setTo] = useState(nowRfc3339());
  const [result, setResult] = useState<UsageResponse | null>(null);
  const [credentials, setCredentials] = useState<CredentialListRow[]>([]);

  const loadCredentials = useCallback(async () => {
    try {
      const data = await request<{ credentials: CredentialListRow[] }>("/admin/credentials", { adminKey });
      setCredentials(data.credentials ?? []);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  }, [adminKey, notify]);

  useEffect(() => {
    void loadCredentials();
  }, [loadCredentials]);

  useEffect(() => {
    if (!provider && providers.length > 0) {
      setProvider(providers[0].name);
    }
  }, [provider, providers]);

  const credentialOptions = useMemo(
    () => credentials.map((item) => ({ value: String(item.id), label: `#${item.id} ${item.name ?? ""}` })),
    [credentials]
  );

  useEffect(() => {
    if (!credentialId && credentialOptions.length > 0) {
      setCredentialId(credentialOptions[0].value);
    }
  }, [credentialId, credentialOptions]);

  const query = async () => {
    try {
      const encodedModel = encodeURIComponent(model.trim());
      let path = "";
      if (mode === "provider") {
        path = `/admin/usage/providers/${provider}/tokens`;
      } else if (mode === "provider_model") {
        path = `/admin/usage/providers/${provider}/models/${encodedModel}/tokens`;
      } else if (mode === "credential") {
        path = `/admin/usage/credentials/${credentialId}/tokens`;
      } else {
        path = `/admin/usage/credentials/${credentialId}/models/${encodedModel}/tokens`;
      }

      const data = await request<UsageResponse>(path, {
        adminKey,
        query: { from, to }
      });
      setResult(data);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const metrics = result
      ? [
        [t("usage.matched_rows"), result.matched_rows],
        [t("usage.call_count"), result.call_count],
        [t("usage.input_tokens"), result.input_tokens],
        [t("usage.output_tokens"), result.output_tokens],
        [t("usage.cache_read_input_tokens"), result.cache_read_input_tokens],
        [t("usage.cache_creation_input_tokens"), result.cache_creation_input_tokens],
        [t("usage.total_tokens"), result.total_tokens]
      ]
    : [];

  return (
    <div className="space-y-5">
      <Card title={t("usage.title")} subtitle={t("usage.subtitle")}>
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          <div>
            <FieldLabel>{t("usage.mode")}</FieldLabel>
            <select className="mt-2 select" value={mode} onChange={(event) => setMode(event.target.value as QueryMode)}>
              <option value="provider">{t("usage.provider_total")}</option>
              <option value="provider_model">{t("usage.provider_model")}</option>
              <option value="credential">{t("usage.credential_total")}</option>
              <option value="credential_model">{t("usage.credential_model")}</option>
            </select>
          </div>

          {(mode === "provider" || mode === "provider_model") && (
            <div>
              <FieldLabel>{t("common.provider")}</FieldLabel>
              <select className="mt-2 select" value={provider} onChange={(event) => setProvider(event.target.value)}>
                {providers.map((item) => (
                  <option key={item.name} value={item.name}>
                    {item.name}
                  </option>
                ))}
              </select>
            </div>
          )}

          {(mode === "credential" || mode === "credential_model") && (
            <div>
              <FieldLabel>Credential ID</FieldLabel>
              <select className="mt-2 select" value={credentialId} onChange={(event) => setCredentialId(event.target.value)}>
                {credentialOptions.map((item) => (
                  <option key={item.value} value={item.value}>
                    {item.label}
                  </option>
                ))}
              </select>
            </div>
          )}

          {(mode === "provider_model" || mode === "credential_model") && (
            <div>
              <FieldLabel>{t("common.model")}</FieldLabel>
              <div className="mt-2">
                <TextInput value={model} onChange={setModel} />
              </div>
            </div>
          )}

          <div>
            <FieldLabel>{t("usage.from")}</FieldLabel>
            <div className="mt-2">
              <TextInput value={from} onChange={setFrom} />
            </div>
          </div>

          <div>
            <FieldLabel>{t("usage.to")}</FieldLabel>
            <div className="mt-2">
              <TextInput value={to} onChange={setTo} />
            </div>
          </div>
        </div>

        <div className="mt-4">
          <Button onClick={() => void query()}>{t("usage.query")}</Button>
        </div>

        {metrics.length > 0 ? (
          <div className="mt-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {metrics.map(([label, value]) => (
              <div key={label} className="metric-card">
                <div className="text-xs uppercase tracking-[0.12em] text-slate-500">{label}</div>
                <div className="mt-2 text-2xl font-semibold text-slate-900">{value}</div>
              </div>
            ))}
          </div>
        ) : null}
      </Card>
    </div>
  );
}

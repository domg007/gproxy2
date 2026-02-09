import { useCallback, useEffect, useMemo, useState } from "react";

import { request, formatApiError } from "../lib/api";
import type { AdminGlobalConfig, ProviderSummary, CredentialListRow, UserRow } from "../lib/types";
import { Card, Button, FieldLabel, TextInput, Badge } from "../components/ui";
import { useI18n } from "../i18n";

type Props = {
  adminKey: string;
  onAdminKeyChange: (nextAdminKey: string) => void;
  notify: (kind: "success" | "error" | "info", message: string) => void;
};

export function OverviewSection({ adminKey, onAdminKeyChange, notify }: Props) {
  const { t } = useI18n();
  const [globalConfig, setGlobalConfig] = useState<AdminGlobalConfig | null>(null);
  const [draft, setDraft] = useState({
    host: "",
    port: "",
    adminKey: "",
    proxy: "",
    eventRedactSensitive: false
  });
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [credentials, setCredentials] = useState<CredentialListRow[]>([]);
  const [users, setUsers] = useState<UserRow[]>([]);
  const [keyCount, setKeyCount] = useState<number>(0);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async (authKey: string = adminKey) => {
    setLoading(true);
    try {
      const [global, providerResp, credentialResp, userResp] = await Promise.all([
        request<AdminGlobalConfig>("/admin/global_config", { adminKey: authKey }),
        request<{ providers: ProviderSummary[] }>("/admin/providers", { adminKey: authKey }),
        request<{ credentials: CredentialListRow[] }>("/admin/credentials", { adminKey: authKey }),
        request<{ users: UserRow[] }>("/admin/users", { adminKey: authKey })
      ]);

      const usersData = userResp.users ?? [];
      const keysByUser = await Promise.all(
        usersData.map((user) =>
          request<{ keys: Array<{ id: number }> }>(`/admin/users/${user.id}/keys`, {
            adminKey: authKey
          })
        )
      );
      const totalKeys = keysByUser.reduce((sum, row) => sum + (row.keys?.length ?? 0), 0);

      setGlobalConfig(global);
      setDraft({
        host: global.host,
        port: String(global.port),
        adminKey: global.admin_key,
        proxy: global.proxy ?? "",
        eventRedactSensitive: Boolean(global.event_redact_sensitive)
      });
      setProviders(providerResp.providers ?? []);
      setCredentials(credentialResp.credentials ?? []);
      setUsers(usersData);
      setKeyCount(totalKeys);
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setLoading(false);
    }
  }, [adminKey, notify]);

  useEffect(() => {
    void load();
  }, [load]);

  const saveGlobal = async () => {
    try {
      const port = Number(draft.port);
      if (Number.isNaN(port)) {
        throw new Error(t("errors.invalid_number"));
      }
      const nextAdminKey = draft.adminKey.trim();
      if (!nextAdminKey) {
        throw new Error(t("auth.required"));
      }
      const changedAdminKey = nextAdminKey !== adminKey;
      await request("/admin/global_config", {
        method: "PUT",
        adminKey,
        body: {
          host: draft.host.trim(),
          port,
          admin_key: nextAdminKey,
          proxy: draft.proxy.trim() || null,
          event_redact_sensitive: draft.eventRedactSensitive
        }
      });
      if (changedAdminKey) {
        onAdminKeyChange(nextAdminKey);
      }
      notify("success", t("overview.global_saved"));
      await load(changedAdminKey ? nextAdminKey : adminKey);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  };

  const cards = useMemo(
    () => [
      { label: t("overview.providers"), value: providers.length },
      { label: t("overview.credentials"), value: credentials.length },
      { label: t("overview.users"), value: users.length },
      { label: t("overview.keys"), value: keyCount }
    ],
    [credentials.length, keyCount, providers.length, t, users.length]
  );

  return (
    <div className="space-y-5">
      <Card
        title={t("overview.title")}
        subtitle={t("overview.subtitle")}
        action={
          <Button variant="neutral" onClick={() => void load()} disabled={loading}>
            {t("common.refresh")}
          </Button>
        }
      >
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {cards.map((item) => (
            <div key={item.label} className="metric-card">
              <div className="text-xs uppercase tracking-[0.12em] text-slate-500">{item.label}</div>
              <div className="mt-2 text-2xl font-semibold text-slate-900">{item.value}</div>
            </div>
          ))}
        </div>
      </Card>

      <Card title="Global Config" subtitle="/admin/global_config">
        <div className="grid gap-4 md:grid-cols-2">
          <div>
            <FieldLabel>{t("overview.host")}</FieldLabel>
            <div className="mt-2">
              <TextInput value={draft.host} onChange={(value) => setDraft((prev) => ({ ...prev, host: value }))} />
            </div>
          </div>
          <div>
            <FieldLabel>{t("overview.port")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                type="number"
                value={draft.port}
                onChange={(value) => setDraft((prev) => ({ ...prev, port: value }))}
              />
            </div>
          </div>
          <div>
            <FieldLabel>{t("overview.proxy")}</FieldLabel>
            <div className="mt-2">
              <TextInput value={draft.proxy} onChange={(value) => setDraft((prev) => ({ ...prev, proxy: value }))} />
            </div>
          </div>
          <div>
            <FieldLabel>{t("overview.admin_key")}</FieldLabel>
            <div className="mt-2">
              <TextInput
                type="password"
                value={draft.adminKey}
                onChange={(value) => setDraft((prev) => ({ ...prev, adminKey: value }))}
              />
            </div>
          </div>
          <div>
            <FieldLabel>{t("overview.dsn")}</FieldLabel>
            <div className="mt-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm text-slate-700">
              {globalConfig?.dsn || "-"}
            </div>
          </div>
          <div className="md:col-span-2 flex items-center gap-2">
            <input
              id="event-redact-sensitive"
              type="checkbox"
              checked={draft.eventRedactSensitive}
              onChange={(event) =>
                setDraft((prev) => ({ ...prev, eventRedactSensitive: event.target.checked }))
              }
            />
            <label htmlFor="event-redact-sensitive" className="text-sm text-slate-700">
              {t("overview.event_redact_sensitive")}
            </label>
            <Badge active={draft.eventRedactSensitive}>
              {draft.eventRedactSensitive ? t("common.enabled") : t("common.disabled")}
            </Badge>
          </div>
        </div>
        <div className="mt-4">
          <Button onClick={() => void saveGlobal()}>{t("common.save")}</Button>
        </div>
      </Card>
    </div>
  );
}

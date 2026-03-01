import { useCallback, useEffect, useMemo, useState } from "react";

import { apiRequest, formatError } from "../../../lib/api";
import { scopeAll } from "../../../lib/scope";
import type {
  CredentialQueryRow,
  ProviderQueryRow,
  UserKeyQueryRow,
  UserQueryRow
} from "../../../lib/types";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

type SelectOption = {
  value: string;
  label: string;
};

function withAllOption(options: SelectOption[], t: TranslateFn): SelectOption[] {
  return [{ value: "", label: t("common.all") }, ...options];
}

function previewApiKey(apiKey: string): string {
  const trimmed = apiKey.trim();
  if (!trimmed) {
    return "-";
  }
  if (trimmed.length <= 14) {
    return trimmed;
  }
  return `${trimmed.slice(0, 6)}...${trimmed.slice(-4)}`;
}

export function useAdminFilterOptions({
  apiKey,
  notify,
  t
}: {
  apiKey: string;
  notify: NotifyFn;
  t: TranslateFn;
}) {
  const [providerRows, setProviderRows] = useState<ProviderQueryRow[]>([]);
  const [credentialRows, setCredentialRows] = useState<CredentialQueryRow[]>([]);
  const [userRows, setUserRows] = useState<UserQueryRow[]>([]);
  const [userKeyRows, setUserKeyRows] = useState<UserKeyQueryRow[]>([]);
  const [isLoading, setIsLoading] = useState(false);

  const reloadOptions = useCallback(async () => {
    setIsLoading(true);
    try {
      const [providers, credentials, users, userKeys] = await Promise.all([
        apiRequest<ProviderQueryRow[]>("/admin/providers/query", {
          apiKey,
          method: "POST",
          body: {
            channel: scopeAll<string>(),
            name: scopeAll<string>(),
            enabled: scopeAll<boolean>(),
            limit: 500
          }
        }),
        apiRequest<CredentialQueryRow[]>("/admin/credentials/query", {
          apiKey,
          method: "POST",
          body: {
            provider_id: scopeAll<number>(),
            kind: scopeAll<string>(),
            enabled: scopeAll<boolean>(),
            limit: 1000
          }
        }),
        apiRequest<UserQueryRow[]>("/admin/users/query", {
          apiKey,
          method: "POST",
          body: {
            id: scopeAll<number>(),
            name: scopeAll<string>()
          }
        }),
        apiRequest<UserKeyQueryRow[]>("/admin/user-keys/query", {
          apiKey,
          method: "POST",
          body: {
            id: scopeAll<number>(),
            user_id: scopeAll<number>(),
            api_key: scopeAll<string>()
          }
        })
      ]);

      setProviderRows([...providers].sort((a, b) => a.id - b.id));
      setCredentialRows([...credentials].sort((a, b) => a.id - b.id));
      setUserRows([...users].sort((a, b) => a.id - b.id));
      setUserKeyRows([...userKeys].sort((a, b) => a.id - b.id));
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      setIsLoading(false);
    }
  }, [apiKey, notify]);

  useEffect(() => {
    void reloadOptions();
  }, [reloadOptions]);

  const providerById = useMemo(
    () => new Map(providerRows.map((row) => [row.id, row])),
    [providerRows]
  );

  const userById = useMemo(() => new Map(userRows.map((row) => [row.id, row])), [userRows]);

  const providerOptions = useMemo(
    () =>
      withAllOption(
        providerRows.map((row) => ({
          value: String(row.id),
          label: `#${row.id} · ${row.channel} · ${row.name || "-"}`
        })),
        t
      ),
    [providerRows, t]
  );

  const credentialOptions = useMemo(
    () =>
      withAllOption(
        credentialRows.map((row) => {
          const provider = providerById.get(row.provider_id);
          const providerMeta = provider
            ? `${provider.channel} (#${provider.id})`
            : `provider #${row.provider_id}`;
          return {
            value: String(row.id),
            label: `#${row.id} · ${row.name?.trim() || t("providers.credentialUnnamed")} · ${providerMeta}`
          };
        }),
        t
      ),
    [credentialRows, providerById, t]
  );

  const userOptions = useMemo(
    () =>
      withAllOption(
        userRows.map((row) => ({
          value: String(row.id),
          label: `#${row.id} · ${row.name}`
        })),
        t
      ),
    [userRows, t]
  );

  const userKeyOptions = useMemo(
    () =>
      withAllOption(
        userKeyRows.map((row) => {
          const user = userById.get(row.user_id);
          const userMeta = user ? `${user.name} (#${row.user_id})` : `user #${row.user_id}`;
          return {
            value: String(row.id),
            label: `#${row.id} · ${userMeta} · ${previewApiKey(row.api_key)}`
          };
        }),
        t
      ),
    [t, userById, userKeyRows]
  );

  return {
    isLoading,
    reloadOptions,
    providerOptions,
    credentialOptions,
    userOptions,
    userKeyOptions
  };
}

import { useCallback, useEffect, useMemo, useState } from "react";

import { apiRequest, formatError } from "../../../../lib/api";
import { scopeAll, scopeEq } from "../../../../lib/scope";
import type {
  CredentialQueryRow,
  CredentialStatusQueryRow,
  ProviderQueryRow
} from "../../../../lib/types";
import { createEmptyProviderFormState, toProviderFormState } from "../index";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;

export function useProviderData({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: NotifyFn;
}) {
  const [providerRows, setProviderRows] = useState<ProviderQueryRow[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<number | null>(null);
  const [providerForm, setProviderForm] = useState(createEmptyProviderFormState);
  const [credentialRows, setCredentialRows] = useState<CredentialQueryRow[]>([]);
  const [statusRows, setStatusRows] = useState<CredentialStatusQueryRow[]>([]);

  const selectedProvider = useMemo(
    () => providerRows.find((row) => row.id === selectedProviderId) ?? null,
    [providerRows, selectedProviderId]
  );

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

  const beginCreateProvider = useCallback(() => {
    setProviderForm(createEmptyProviderFormState());
    setSelectedProviderId(null);
  }, []);

  const selectProvider = useCallback((row: ProviderQueryRow) => {
    setProviderForm(toProviderFormState(row));
    setSelectedProviderId(row.id);
  }, []);

  return {
    providerRows,
    setProviderRows,
    selectedProviderId,
    setSelectedProviderId,
    providerForm,
    setProviderForm,
    credentialRows,
    setCredentialRows,
    statusRows,
    setStatusRows,
    selectedProvider,
    statusesByCredential,
    loadProviders,
    loadProviderScopedData,
    beginCreateProvider,
    selectProvider
  };
}

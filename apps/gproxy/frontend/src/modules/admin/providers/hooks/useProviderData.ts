import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { apiRequest, formatError } from "../../../../lib/api";
import { scopeAll, scopeEq, scopeIn } from "../../../../lib/scope";
import type {
  CredentialQueryCount,
  CredentialQueryRow,
  CredentialStatusQueryCount,
  CredentialStatusQueryRow,
  ProviderChannelCatalogRow,
  ProviderQueryRow
} from "../../../../lib/types";
import { createEmptyProviderFormState, toProviderFormState } from "../index";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;

export type CredentialSearchMode = "id" | "name";

const FIRST_PAGE = 1;

function defaultCredentialPageSize(): number {
  if (typeof window === "undefined") {
    return 20;
  }
  if (window.innerWidth < 640) {
    return 5;
  }
  if (window.innerWidth < 1024) {
    return 10;
  }
  if (window.innerWidth < 1600) {
    return 20;
  }
  return 50;
}

function resolveCredentialSearchFilter(searchMode: CredentialSearchMode, rawSearchText: string): {
  exactId: number | null;
  invalidIdSearch: boolean;
  nameContains: string | null;
} {
  const searchText = rawSearchText.trim();
  if (!searchText) {
    return {
      exactId: null,
      invalidIdSearch: false,
      nameContains: null
    };
  }
  if (searchMode === "name") {
    return {
      exactId: null,
      invalidIdSearch: false,
      nameContains: searchText
    };
  }
  if (!/^\d+$/.test(searchText)) {
    return {
      exactId: null,
      invalidIdSearch: true,
      nameContains: null
    };
  }
  const parsed = Number.parseInt(searchText, 10);
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    return {
      exactId: null,
      invalidIdSearch: true,
      nameContains: null
    };
  }
  return {
    exactId: parsed,
    invalidIdSearch: false,
    nameContains: null
  };
}

export function useProviderData({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: NotifyFn;
}) {
  const [providerRows, setProviderRows] = useState<ProviderQueryRow[]>([]);
  const [channelCatalogRows, setChannelCatalogRows] = useState<ProviderChannelCatalogRow[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<number | null>(null);
  const [providerForm, setProviderForm] = useState(createEmptyProviderFormState);
  const [credentialRows, setCredentialRows] = useState<CredentialQueryRow[]>([]);
  const [statusRows, setStatusRows] = useState<CredentialStatusQueryRow[]>([]);
  const [credentialSearchMode, setCredentialSearchMode] = useState<CredentialSearchMode>("name");
  const [credentialSearchText, setCredentialSearchText] = useState("");
  const [credentialPageSize, setCredentialPageSize] = useState<number>(
    () => defaultCredentialPageSize()
  );
  const [credentialPage, setCredentialPage] = useState(FIRST_PAGE);
  const [credentialTotalCount, setCredentialTotalCount] = useState(0);
  const [credentialFilteredCount, setCredentialFilteredCount] = useState(0);
  const [deadCredentialCount, setDeadCredentialCount] = useState(0);
  const [credentialListLoading, setCredentialListLoading] = useState(false);
  const credentialLoadRequestIdRef = useRef(0);

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

  const resetCredentialListView = useCallback(() => {
    setCredentialSearchMode("name");
    setCredentialSearchText("");
    setCredentialPageSize(defaultCredentialPageSize());
    setCredentialPage(FIRST_PAGE);
  }, []);

  const loadChannelCatalog = useCallback(async () => {
    try {
      const data = await apiRequest<ProviderChannelCatalogRow[]>("/admin/providers/catalog", {
        apiKey
      });
      setChannelCatalogRows(data);
      return data;
    } catch (error) {
      notify("error", formatError(error));
      return [];
    }
  }, [apiKey, notify]);

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
      return sorted;
    } catch (error) {
      notify("error", formatError(error));
      return [];
    }
  }, [apiKey, notify]);

  const loadAllProviderCredentials = useCallback(
    async (provider: ProviderQueryRow | null) => {
      if (!provider) {
        return [];
      }
      try {
        return await apiRequest<CredentialQueryRow[]>("/admin/credentials/query", {
          apiKey,
          method: "POST",
          body: {
            id: scopeAll<number>(),
            provider_id: scopeEq(provider.id),
            kind: scopeAll<string>(),
            enabled: scopeAll<boolean>(),
            name_contains: null,
            limit: 0,
            offset: 0
          }
        });
      } catch (error) {
        notify("error", formatError(error));
        return [];
      }
    },
    [apiKey, notify]
  );

  const loadDeadCredentialIds = useCallback(
    async (provider: ProviderQueryRow | null) => {
      if (!provider) {
        return [];
      }
      try {
        const rows = await apiRequest<CredentialStatusQueryRow[]>("/admin/credential-statuses/query", {
          apiKey,
          method: "POST",
          body: {
            id: scopeAll<number>(),
            credential_id: scopeAll<number>(),
            provider_id: scopeEq(provider.id),
            channel: scopeEq(provider.channel),
            health_kind: scopeEq("dead"),
            limit: 0,
            offset: 0
          }
        });
        return rows.map((row) => row.credential_id);
      } catch (error) {
        notify("error", formatError(error));
        return [];
      }
    },
    [apiKey, notify]
  );

  const loadProviderScopedData = useCallback(
    async (provider: ProviderQueryRow | null) => {
      const requestId = credentialLoadRequestIdRef.current + 1;
      credentialLoadRequestIdRef.current = requestId;

      if (!provider) {
        setCredentialRows([]);
        setStatusRows([]);
        setCredentialTotalCount(0);
        setCredentialFilteredCount(0);
        setDeadCredentialCount(0);
        setCredentialListLoading(false);
        return { credentials: [], statuses: [] };
      }

      setCredentialListLoading(true);

      const searchFilter = resolveCredentialSearchFilter(
        credentialSearchMode,
        credentialSearchText
      );
      const baseCredentialQuery = {
        id:
          searchFilter.exactId === null
            ? scopeAll<number>()
            : scopeEq(searchFilter.exactId),
        provider_id: scopeEq(provider.id),
        kind: scopeAll<string>(),
        enabled: scopeAll<boolean>(),
        name_contains: searchFilter.nameContains,
        limit: 0,
        offset: 0
      };

      try {
        const totalCountPromise = apiRequest<CredentialQueryCount>("/admin/credentials/count", {
          apiKey,
          method: "POST",
          body: {
            id: scopeAll<number>(),
            provider_id: scopeEq(provider.id),
            kind: scopeAll<string>(),
            enabled: scopeAll<boolean>(),
            name_contains: null,
            limit: 0,
            offset: 0
          }
        });
        const deadCountPromise = apiRequest<CredentialStatusQueryCount>(
          "/admin/credential-statuses/count",
          {
            apiKey,
            method: "POST",
            body: {
              id: scopeAll<number>(),
              credential_id: scopeAll<number>(),
              provider_id: scopeEq(provider.id),
              channel: scopeEq(provider.channel),
              health_kind: scopeEq("dead"),
              limit: 0,
              offset: 0
            }
          }
        );

        if (searchFilter.invalidIdSearch) {
          const [totalCount, deadCount] = await Promise.all([totalCountPromise, deadCountPromise]);
          if (credentialLoadRequestIdRef.current !== requestId) {
            return { credentials: [], statuses: [] };
          }
          setCredentialRows([]);
          setStatusRows([]);
          setCredentialTotalCount(totalCount.count);
          setCredentialFilteredCount(0);
          setDeadCredentialCount(deadCount.count);
          return { credentials: [], statuses: [] };
        }

        const filteredCountPromise = apiRequest<CredentialQueryCount>("/admin/credentials/count", {
          apiKey,
          method: "POST",
          body: baseCredentialQuery
        });
        const [totalCount, filteredCount, deadCount] = await Promise.all([
          totalCountPromise,
          filteredCountPromise,
          deadCountPromise
        ]);
        const totalPages = Math.max(1, Math.ceil(filteredCount.count / credentialPageSize));
        const effectivePage = Math.min(credentialPage, totalPages);
        const credentials = await apiRequest<CredentialQueryRow[]>("/admin/credentials/query", {
          apiKey,
          method: "POST",
          body: {
            ...baseCredentialQuery,
            limit: credentialPageSize,
            offset: (effectivePage - 1) * credentialPageSize
          }
        });
        const credentialIds = credentials.map((row) => row.id);
        const statuses =
          credentialIds.length === 0
            ? []
            : await apiRequest<CredentialStatusQueryRow[]>("/admin/credential-statuses/query", {
                apiKey,
                method: "POST",
                body: {
                  id: scopeAll<number>(),
                  credential_id: scopeIn(credentialIds),
                  provider_id: scopeEq(provider.id),
                  channel: scopeEq(provider.channel),
                  health_kind: scopeAll<string>(),
                  limit: 0,
                  offset: 0
                }
              });

        if (credentialLoadRequestIdRef.current !== requestId) {
          return { credentials, statuses };
        }

        setCredentialRows(credentials);
        setStatusRows(statuses);
        setCredentialTotalCount(totalCount.count);
        setCredentialFilteredCount(filteredCount.count);
        setDeadCredentialCount(deadCount.count);
        if (effectivePage !== credentialPage) {
          setCredentialPage(effectivePage);
        }
        return { credentials, statuses };
      } catch (error) {
        if (credentialLoadRequestIdRef.current === requestId) {
          notify("error", formatError(error));
        }
        return { credentials: [], statuses: [] };
      } finally {
        if (credentialLoadRequestIdRef.current === requestId) {
          setCredentialListLoading(false);
        }
      }
    },
    [
      apiKey,
      credentialPage,
      credentialPageSize,
      credentialSearchMode,
      credentialSearchText,
      notify
    ]
  );

  useEffect(() => {
    void loadChannelCatalog();
    void loadProviders();
  }, [loadChannelCatalog, loadProviders]);

  useEffect(() => {
    void loadProviderScopedData(selectedProvider);
  }, [loadProviderScopedData, selectedProvider]);

  const beginCreateProvider = useCallback(() => {
    setProviderForm(createEmptyProviderFormState());
    setSelectedProviderId(null);
    resetCredentialListView();
  }, [resetCredentialListView]);

  const selectProvider = useCallback(
    (row: ProviderQueryRow) => {
      setProviderForm(toProviderFormState(row));
      setSelectedProviderId(row.id);
      resetCredentialListView();
    },
    [resetCredentialListView]
  );

  const updateCredentialSearchMode = useCallback((value: CredentialSearchMode) => {
    setCredentialSearchMode(value);
    setCredentialPage(FIRST_PAGE);
  }, []);

  const updateCredentialSearchText = useCallback((value: string) => {
    setCredentialSearchText(value);
    setCredentialPage(FIRST_PAGE);
  }, []);

  const updateCredentialPageSize = useCallback((value: number) => {
    setCredentialPageSize(value);
    setCredentialPage(FIRST_PAGE);
  }, []);

  return {
    providerRows,
    setProviderRows,
    channelCatalogRows,
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
    credentialSearchMode,
    credentialSearchText,
    credentialPageSize,
    credentialPage,
    credentialTotalCount,
    credentialFilteredCount,
    deadCredentialCount,
    credentialListLoading,
    setCredentialPage,
    updateCredentialSearchMode,
    updateCredentialSearchText,
    updateCredentialPageSize,
    loadProviders,
    loadProviderScopedData,
    loadAllProviderCredentials,
    loadDeadCredentialIds,
    resetCredentialListView,
    beginCreateProvider,
    selectProvider
  };
}

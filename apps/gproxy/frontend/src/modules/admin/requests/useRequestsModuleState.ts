import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { apiRequest, formatError } from "../../../lib/api";
import { parseDateTimeLocalToUnixMs } from "../../../lib/datetime";
import { parseOptionalI64 } from "../../../lib/form";
import { scopeAll, scopeEq } from "../../../lib/scope";
import type {
  DownstreamRequestQueryRow,
  RequestClearAck,
  RequestQueryCount,
  UpstreamRequestQueryRow
} from "../../../lib/types";
import { useAdminFilterOptions } from "../hooks/useAdminFilterOptions";
import type {
  RequestBodyPayload,
  RequestsFilterState,
  RequestKind,
  RequestQuerySnapshot,
  RequestRow,
  SelectOption,
  NotifyFn,
  TranslateFn
} from "./types";

const REQUEST_PATH_TEMPLATE_OPTIONS = [
  "/v1/messages",
  "/v1/chat/completions",
  "/v1/responses",
  "/v1/models",
  "/v1/embeddings",
  "/v1/usage",
  "/healthz",
  "/admin/"
] as const;

const DEFAULT_FILTERS: RequestsFilterState = {
  providerId: "",
  credentialId: "",
  userId: "",
  userKeyId: "",
  requestPathContains: "",
  fromAt: "",
  toAt: "",
  limit: "100"
};

function defaultPageSizeByViewport(): number {
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

function buildRequestCountPayload(snapshot: RequestQuerySnapshot) {
  if (snapshot.kind === "upstream") {
    return {
      trace_id: scopeAll<number>(),
      provider_id: snapshot.providerId === null ? scopeAll<number>() : scopeEq(snapshot.providerId),
      credential_id:
        snapshot.credentialId === null ? scopeAll<number>() : scopeEq(snapshot.credentialId),
      request_url_contains: snapshot.pathContains || null,
      from_unix_ms: snapshot.fromUnixMs,
      to_unix_ms: snapshot.toUnixMs
    };
  }

  return {
    trace_id: scopeAll<number>(),
    user_id: snapshot.userId === null ? scopeAll<number>() : scopeEq(snapshot.userId),
    user_key_id: snapshot.userKeyId === null ? scopeAll<number>() : scopeEq(snapshot.userKeyId),
    request_path_contains: snapshot.pathContains || null,
    from_unix_ms: snapshot.fromUnixMs,
    to_unix_ms: snapshot.toUnixMs
  };
}

function buildRequestRowsPayload(
  snapshot: RequestQuerySnapshot,
  options: {
    offset: number;
    limit: number;
    includeBody: boolean;
    traceId?: number;
  }
) {
  if (snapshot.kind === "upstream") {
    return {
      trace_id: options.traceId === undefined ? scopeAll<number>() : scopeEq(options.traceId),
      provider_id: snapshot.providerId === null ? scopeAll<number>() : scopeEq(snapshot.providerId),
      credential_id:
        snapshot.credentialId === null ? scopeAll<number>() : scopeEq(snapshot.credentialId),
      request_url_contains: snapshot.pathContains || null,
      from_unix_ms: snapshot.fromUnixMs,
      to_unix_ms: snapshot.toUnixMs,
      offset: options.offset,
      limit: options.limit,
      include_body: options.includeBody
    };
  }

  return {
    trace_id: options.traceId === undefined ? scopeAll<number>() : scopeEq(options.traceId),
    user_id: snapshot.userId === null ? scopeAll<number>() : scopeEq(snapshot.userId),
    user_key_id: snapshot.userKeyId === null ? scopeAll<number>() : scopeEq(snapshot.userKeyId),
    request_path_contains: snapshot.pathContains || null,
    from_unix_ms: snapshot.fromUnixMs,
    to_unix_ms: snapshot.toUnixMs,
    offset: options.offset,
    limit: options.limit,
    include_body: options.includeBody
  };
}

function requestRowsPath(kind: RequestKind): string {
  return kind === "upstream" ? "/admin/requests/upstream/query" : "/admin/requests/downstream/query";
}

function requestCountPath(kind: RequestKind): string {
  return kind === "upstream" ? "/admin/requests/upstream/count" : "/admin/requests/downstream/count";
}

function requestClearPath(kind: RequestKind): string {
  return kind === "upstream" ? "/admin/requests/upstream/clear" : "/admin/requests/downstream/clear";
}

function toPositiveOrNull(value: number | null): number | null {
  if (value === null || value <= 0) {
    return null;
  }
  return value;
}

export function useRequestsModuleState({
  apiKey,
  notify,
  t
}: {
  apiKey: string;
  notify: NotifyFn;
  t: TranslateFn;
}) {
  const [kind, setKind] = useState<RequestKind>("upstream");
  const [filters, setFilters] = useState<RequestsFilterState>(DEFAULT_FILTERS);
  const [rows, setRows] = useState<RequestRow[]>([]);
  const [pageSize, setPageSize] = useState<number>(() => defaultPageSizeByViewport());
  const [page, setPage] = useState(1);
  const [totalRows, setTotalRows] = useState(0);
  const [activeQuery, setActiveQuery] = useState<RequestQuerySnapshot | null>(null);
  const [loadingRows, setLoadingRows] = useState(false);
  const [loadingCount, setLoadingCount] = useState(false);
  const [knownRequestPaths, setKnownRequestPaths] = useState<string[]>([]);
  const [bodyByTraceId, setBodyByTraceId] = useState<Record<number, RequestBodyPayload>>({});
  const [bodyLoadingByTraceId, setBodyLoadingByTraceId] = useState<Record<number, boolean>>({});
  const [bodyErrorByTraceId, setBodyErrorByTraceId] = useState<Record<number, string>>({});
  const [selectedTraceIds, setSelectedTraceIds] = useState<number[]>([]);
  const [clearingPayload, setClearingPayload] = useState(false);
  const rowsRequestSeq = useRef(0);
  const countRequestSeq = useRef(0);

  const {
    isLoading: isFilterOptionsLoading,
    providerRows,
    credentialRows,
    userRows,
    userKeyRows,
    providerOptions,
    userOptions
  } = useAdminFilterOptions({
    apiKey,
    notify,
    t
  });

  const selectedProviderId = useMemo(() => {
    const value = Number(filters.providerId);
    return Number.isInteger(value) ? value : null;
  }, [filters.providerId]);

  const selectedUserId = useMemo(() => {
    const value = Number(filters.userId);
    return Number.isInteger(value) ? value : null;
  }, [filters.userId]);

  const providerById = useMemo(
    () => new Map(providerRows.map((row) => [row.id, row])),
    [providerRows]
  );

  const userById = useMemo(() => new Map(userRows.map((row) => [row.id, row])), [userRows]);

  const filteredCredentialOptions = useMemo<SelectOption[]>(() => {
    const scopedRows =
      selectedProviderId === null
        ? credentialRows
        : credentialRows.filter((row) => row.provider_id === selectedProviderId);
    return [
      { value: "", label: t("common.all") },
      ...scopedRows.map((row) => {
        const provider = providerById.get(row.provider_id);
        const providerMeta = provider
          ? `${provider.channel} (#${provider.id})`
          : `provider #${row.provider_id}`;
        return {
          value: String(row.id),
          label: `#${row.id} · ${row.name?.trim() || t("providers.credentialUnnamed")} · ${providerMeta}`
        };
      })
    ];
  }, [credentialRows, providerById, selectedProviderId, t]);

  const filteredUserKeyOptions = useMemo<SelectOption[]>(() => {
    const scopedRows =
      selectedUserId === null
        ? userKeyRows
        : userKeyRows.filter((row) => row.user_id === selectedUserId);
    return [
      { value: "", label: t("common.all") },
      ...scopedRows.map((row) => {
        const user = userById.get(row.user_id);
        const userMeta = user ? `${user.name} (#${row.user_id})` : `user #${row.user_id}`;
        const key = row.api_key.trim();
        const preview = key.length <= 14 ? key : `${key.slice(0, 6)}...${key.slice(-4)}`;
        return {
          value: String(row.id),
          label: `#${row.id} · ${userMeta} · ${preview}`
        };
      })
    ];
  }, [selectedUserId, t, userById, userKeyRows]);

  const requestPathOptions = useMemo<SelectOption[]>(() => {
    const dynamic = knownRequestPaths.filter((item) => item.trim().length > 0);
    const combined = Array.from(new Set<string>([...REQUEST_PATH_TEMPLATE_OPTIONS, ...dynamic])).sort(
      (a, b) => a.localeCompare(b)
    );
    return [
      { value: "", label: t("common.all") },
      ...combined.map((value) => ({ value, label: value }))
    ];
  }, [knownRequestPaths, t]);

  useEffect(() => {
    if (!filters.credentialId) {
      return;
    }
    const credentialId = Number(filters.credentialId);
    const exists = filteredCredentialOptions.some((item) => Number(item.value) === credentialId);
    if (!exists) {
      setFilters((prev) => ({ ...prev, credentialId: "" }));
    }
  }, [filteredCredentialOptions, filters.credentialId]);

  useEffect(() => {
    if (!filters.userKeyId) {
      return;
    }
    const userKeyId = Number(filters.userKeyId);
    const exists = filteredUserKeyOptions.some((item) => Number(item.value) === userKeyId);
    if (!exists) {
      setFilters((prev) => ({ ...prev, userKeyId: "" }));
    }
  }, [filteredUserKeyOptions, filters.userKeyId]);

  useEffect(() => {
    setActiveQuery(null);
    setRows([]);
    setTotalRows(0);
    setPage(1);
    setKnownRequestPaths([]);
    setBodyByTraceId({});
    setBodyLoadingByTraceId({});
    setBodyErrorByTraceId({});
    setSelectedTraceIds([]);
  }, [kind]);

  useEffect(() => {
    setPage(1);
  }, [pageSize]);

  const totalPages = Math.max(1, Math.ceil(totalRows / pageSize));

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  const buildSnapshot = useCallback((): RequestQuerySnapshot => {
    const providerId = parseOptionalI64(filters.providerId);
    const credentialId = parseOptionalI64(filters.credentialId);
    const userId = parseOptionalI64(filters.userId);
    const userKeyId = parseOptionalI64(filters.userKeyId);
    const pathContains = filters.requestPathContains.trim();
    const fromUnixMs = parseDateTimeLocalToUnixMs(filters.fromAt);
    const toUnixMs = parseDateTimeLocalToUnixMs(filters.toAt);
    const maxRows = toPositiveOrNull(parseOptionalI64(filters.limit));
    return {
      kind,
      providerId,
      credentialId,
      userId,
      userKeyId,
      pathContains,
      fromUnixMs,
      toUnixMs,
      maxRows
    };
  }, [filters, kind]);

  const runQuery = useCallback(() => {
    const snapshot = buildSnapshot();
    setActiveQuery(snapshot);
    setPage(1);
    setRows([]);
    setTotalRows(0);
    setBodyByTraceId({});
    setBodyLoadingByTraceId({});
    setBodyErrorByTraceId({});
    setSelectedTraceIds([]);
  }, [buildSnapshot]);

  useEffect(() => {
    if (!activeQuery) {
      return;
    }
    const requestId = ++countRequestSeq.current;
    setLoadingCount(true);
    const fetchCount = async () => {
      try {
        const countResult = await apiRequest<RequestQueryCount>(requestCountPath(activeQuery.kind), {
          apiKey,
          method: "POST",
          body: buildRequestCountPayload(activeQuery)
        });
        if (requestId !== countRequestSeq.current) {
          return;
        }
        const maxRows = activeQuery.maxRows;
        const nextTotal = maxRows === null ? countResult.count : Math.min(countResult.count, maxRows);
        setTotalRows(nextTotal);
      } catch (error) {
        notify("error", formatError(error));
      } finally {
        if (requestId === countRequestSeq.current) {
          setLoadingCount(false);
        }
      }
    };
    void fetchCount();
  }, [activeQuery, apiKey, notify]);

  useEffect(() => {
    if (!activeQuery) {
      return;
    }
    const offset = (page - 1) * pageSize;
    const maxRows = activeQuery.maxRows;
    const remaining = maxRows === null ? pageSize : Math.max(0, maxRows - offset);
    const limit = Math.min(pageSize, remaining);
    if (limit <= 0) {
      setRows([]);
      return;
    }

    const requestId = ++rowsRequestSeq.current;
    setLoadingRows(true);
    const fetchRows = async () => {
      try {
        const data = await apiRequest<RequestRow[]>(requestRowsPath(activeQuery.kind), {
          apiKey,
          method: "POST",
          body: buildRequestRowsPayload(activeQuery, {
            offset,
            limit,
            includeBody: false
          })
        });
        if (requestId !== rowsRequestSeq.current) {
          return;
        }
        setRows(data);

        const paths =
          activeQuery.kind === "downstream"
            ? data
                .map((row) => (row as DownstreamRequestQueryRow).request_path?.trim() ?? "")
                .filter((value) => value.length > 0)
            : data
                .map((row) => (row as UpstreamRequestQueryRow).request_url?.trim() ?? "")
                .filter((value) => value.length > 0);
        if (paths.length > 0) {
          setKnownRequestPaths((prev) => Array.from(new Set([...prev, ...paths])).sort());
        }
      } catch (error) {
        notify("error", formatError(error));
      } finally {
        if (requestId === rowsRequestSeq.current) {
          setLoadingRows(false);
        }
      }
    };
    void fetchRows();
  }, [activeQuery, apiKey, notify, page, pageSize]);

  const ensureBodyLoaded = useCallback(async (row: RequestRow): Promise<RequestBodyPayload | undefined> => {
    if (!activeQuery) {
      return undefined;
    }
    const traceId = row.trace_id;
    if (bodyByTraceId[traceId] || bodyLoadingByTraceId[traceId]) {
      return bodyByTraceId[traceId];
    }

    setBodyLoadingByTraceId((prev) => ({ ...prev, [traceId]: true }));
    setBodyErrorByTraceId((prev) => {
      const next = { ...prev };
      delete next[traceId];
      return next;
    });

    try {
      const data = await apiRequest<RequestRow[]>(requestRowsPath(activeQuery.kind), {
        apiKey,
        method: "POST",
        body: buildRequestRowsPayload(activeQuery, {
          traceId,
          offset: 0,
          limit: 1,
          includeBody: true
        })
      });
      const detail = data[0];
      const nextDetail = {
        request_body: detail?.request_body ?? null,
        response_body: detail?.response_body ?? null
      };
      setBodyByTraceId((prev) => ({
        ...prev,
        [traceId]: nextDetail
      }));
      return nextDetail;
    } catch (error) {
      setBodyErrorByTraceId((prev) => ({
        ...prev,
        [traceId]: formatError(error)
      }));
      return undefined;
    } finally {
      setBodyLoadingByTraceId((prev) => ({ ...prev, [traceId]: false }));
    }
  }, [activeQuery, apiKey, bodyByTraceId, bodyLoadingByTraceId]);

  const toggleTraceIdSelected = useCallback((traceId: number) => {
    setSelectedTraceIds((prev) =>
      prev.includes(traceId) ? prev.filter((item) => item !== traceId) : [...prev, traceId]
    );
  }, []);

  const clearPayload = useCallback(async (all: boolean) => {
    const normalizedIds = Array.from(
      new Set(selectedTraceIds.filter((id) => Number.isInteger(id) && id > 0))
    ).sort((a, b) => a - b);

    if (!all && normalizedIds.length === 0) {
      notify("info", t("common.none"));
      return;
    }

    const confirmed = all
      ? window.confirm(t("requests.clear.confirmAll"))
      : window.confirm(t("requests.clear.confirmSelected", { count: normalizedIds.length }));
    if (!confirmed) {
      return;
    }

    setClearingPayload(true);
    try {
      const result = await apiRequest<RequestClearAck>(requestClearPath(kind), {
        apiKey,
        method: "POST",
        body: {
          all,
          trace_ids: all ? [] : normalizedIds
        }
      });
      notify("success", t("requests.clear.done", { count: result.cleared }));
      setSelectedTraceIds([]);
      setBodyByTraceId({});
      setBodyLoadingByTraceId({});
      setBodyErrorByTraceId({});

      if (activeQuery) {
        setActiveQuery({ ...activeQuery });
      } else {
        setRows([]);
        setTotalRows(0);
      }
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      setClearingPayload(false);
    }
  }, [activeQuery, apiKey, kind, notify, selectedTraceIds, t]);

  const updateFilter = useCallback((key: keyof RequestsFilterState, value: string) => {
    setFilters((prev) => ({ ...prev, [key]: value }));
  }, []);

  return {
    kind,
    setKind,
    filters,
    updateFilter,
    rows,
    pageSize,
    setPageSize,
    page,
    setPage,
    totalRows,
    totalPages,
    loadingRows,
    loadingCount,
    clearingPayload,
    selectedTraceIds,
    bodyByTraceId,
    bodyLoadingByTraceId,
    bodyErrorByTraceId,
    isFilterOptionsLoading,
    providerOptions,
    filteredCredentialOptions,
    userOptions,
    filteredUserKeyOptions,
    requestPathOptions,
    runQuery,
    ensureBodyLoaded,
    toggleTraceIdSelected,
    clearPayload
  };
}

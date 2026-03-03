import { useEffect, useMemo, useRef, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { copyTextToClipboard } from "../../lib/clipboard";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type {
  DownstreamRequestQueryRow,
  RequestQueryCount,
  UpstreamRequestQueryRow
} from "../../lib/types";
import { Button, Card, Input, Label, SearchableSelect, Select, Table } from "../../components/ui";
import { useAdminFilterOptions } from "./hooks/useAdminFilterOptions";

type RequestKind = "upstream" | "downstream";
type RequestRow = UpstreamRequestQueryRow | DownstreamRequestQueryRow;

type RequestQuerySnapshot = {
  kind: RequestKind;
  providerId: number | null;
  credentialId: number | null;
  userId: number | null;
  userKeyId: number | null;
  pathContains: string;
  fromUnixMs: number | null;
  toUnixMs: number | null;
  maxRows: number | null;
};

type RequestBodyPayload = {
  request_body: number[] | null;
  response_body: number[] | null;
};

type PayloadPreview = {
  preview: string;
  full: string;
  truncated: boolean;
};

const META_DEFAULT_PREVIEW_CHARS = 420;
const REQUEST_PATH_TEMPLATE_OPTIONS = [
  "/v1/messages",
  "/v1/chat/completions",
  "/v1/responses",
  "/v1/models",
  "/v1/embeddings",
  "/v1/usage",
  "/healthz",
  "/admin/"
];

function EyeToggleIcon({ open }: { open: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      aria-hidden="true"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z" />
      {open ? <circle cx="12" cy="12" r="2.5" /> : <path d="M4 20L20 4" />}
    </svg>
  );
}

function BodyEyeButton({
  ariaLabel,
  open,
  loading,
  onClick
}: {
  ariaLabel: string;
  open: boolean;
  loading: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="inline-flex cursor-pointer items-center text-muted hover:text-text disabled:cursor-not-allowed disabled:opacity-60"
      onClick={onClick}
      aria-label={ariaLabel}
      disabled={loading}
    >
      <EyeToggleIcon open={open} />
    </button>
  );
}

function BodyCopyButton({
  ariaLabel,
  loading,
  onClick
}: {
  ariaLabel: string;
  loading: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="relative z-10 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border bg-panel-muted text-muted transition hover:text-text disabled:cursor-not-allowed disabled:opacity-60"
      onClick={onClick}
      aria-label={ariaLabel}
      title={ariaLabel}
      disabled={loading}
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        className="h-4 w-4"
        aria-hidden="true"
      >
        <rect x="9" y="9" width="11" height="11" rx="2" />
        <path d="M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
      </svg>
    </button>
  );
}

function PayloadSection({
  title,
  section,
  action
}: {
  title: string;
  section: PayloadPreview;
  action?: React.ReactNode;
}) {
  const [showFull, setShowFull] = useState(false);
  if (!section.preview) {
    return null;
  }
  const canToggle = section.truncated && section.full !== section.preview;
  const content = showFull ? section.full : section.preview;

  return (
    <div>
      <div className="mb-1 flex items-center gap-1 font-semibold text-muted">
        <span>{title}</span>
        {action}
        {canToggle ? (
          <button
            type="button"
            className="inline-flex cursor-pointer items-center text-muted hover:text-text"
            aria-label={`${showFull ? "show truncated" : "show full"} ${title}`}
            onClick={() => setShowFull((value) => !value)}
          >
            <EyeToggleIcon open={showFull} />
          </button>
        ) : null}
      </div>
      <pre className="whitespace-pre-wrap break-all rounded px-2 py-1">{content}</pre>
    </div>
  );
}

function bytesToUtf8Preview(bytes: number[] | null): PayloadPreview {
  if (!bytes || bytes.length === 0) {
    return { preview: "", full: "", truncated: false };
  }
  try {
    const decoded = new TextDecoder().decode(new Uint8Array(bytes));
    return { preview: decoded, full: decoded, truncated: false };
  } catch {
    const binary = `[binary ${bytes.length} bytes]`;
    return { preview: binary, full: binary, truncated: false };
  }
}

function textToPreview(text: string | null | undefined): PayloadPreview {
  if (!text) {
    return { preview: "", full: "", truncated: false };
  }
  if (text.length <= META_DEFAULT_PREVIEW_CHARS) {
    return { preview: text, full: text, truncated: false };
  }
  return {
    preview: text.slice(0, META_DEFAULT_PREVIEW_CHARS),
    full: text,
    truncated: true
  };
}

function jsonToPreview(value: Record<string, unknown>): PayloadPreview {
  const text = JSON.stringify(value);
  if (!text || text === "{}") {
    return { preview: "", full: "", truncated: false };
  }
  return textToPreview(text);
}

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
      credential_id: snapshot.credentialId === null ? scopeAll<number>() : scopeEq(snapshot.credentialId),
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
      credential_id: snapshot.credentialId === null ? scopeAll<number>() : scopeEq(snapshot.credentialId),
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

function toPositiveOrNull(value: number | null): number | null {
  if (value === null || value <= 0) {
    return null;
  }
  return value;
}

function PayloadCell({
  row,
  t,
  notify,
  detail,
  loadingBody,
  bodyError,
  ensureBodyLoaded
}: {
  row: RequestRow;
  t: (key: string, params?: Record<string, string | number>) => string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
  detail: RequestBodyPayload | undefined;
  loadingBody: boolean;
  bodyError: string | undefined;
  ensureBodyLoaded: (row: RequestRow) => Promise<RequestBodyPayload | undefined>;
}) {
  const [showReqBody, setShowReqBody] = useState(false);
  const [showRespBody, setShowRespBody] = useState(false);
  const requestHeaders = jsonToPreview(row.request_headers_json);
  const responseHeaders = jsonToPreview(row.response_headers_json);
  const requestQuery =
    "request_query" in row ? textToPreview((row as DownstreamRequestQueryRow).request_query) : null;
  const requestBody = bytesToUtf8Preview(detail?.request_body ?? null);
  const responseBody = bytesToUtf8Preview(detail?.response_body ?? null);
  const reqBodySection =
    showReqBody && requestBody.preview
      ? requestBody
      : { preview: "-", full: "-", truncated: false as const };
  const respBodySection =
    showRespBody && responseBody.preview
      ? responseBody
      : { preview: "-", full: "-", truncated: false as const };

  const toggleReqBody = () => {
    if (!showReqBody && !detail && !loadingBody) {
      void ensureBodyLoaded(row);
    }
    setShowReqBody((value) => !value);
  };

  const toggleRespBody = () => {
    if (!showRespBody && !detail && !loadingBody) {
      void ensureBodyLoaded(row);
    }
    setShowRespBody((value) => !value);
  };

  const copyReqBody = async () => {
    let loadedDetail = detail;
    if (!loadedDetail && !loadingBody) {
      loadedDetail = await ensureBodyLoaded(row);
    }
    const payload = bytesToUtf8Preview(loadedDetail?.request_body ?? null).full;
    if (!payload) {
      notify("info", t("common.none"));
      return;
    }
    try {
      await copyTextToClipboard(payload);
      notify("success", t("common.copied"));
    } catch {
      notify("error", t("common.copyFailed"));
    }
  };

  const copyRespBody = async () => {
    let loadedDetail = detail;
    if (!loadedDetail && !loadingBody) {
      loadedDetail = await ensureBodyLoaded(row);
    }
    const payload = bytesToUtf8Preview(loadedDetail?.response_body ?? null).full;
    if (!payload) {
      notify("info", t("common.none"));
      return;
    }
    try {
      await copyTextToClipboard(payload);
      notify("success", t("common.copied"));
    } catch {
      notify("error", t("common.copyFailed"));
    }
  };

  return (
    <details>
      <summary className="cursor-pointer text-xs text-muted" aria-label="toggle payload" />
      <div className="mt-2 space-y-2 text-xs">
        {requestQuery ? (
          requestQuery.preview ? (
            <PayloadSection title="req query" section={requestQuery} />
          ) : (
            <div>
              <div className="mb-1 font-semibold text-muted">req query</div>
              <div className="text-xs text-muted">-</div>
            </div>
          )
        ) : null}
        <PayloadSection title="req headers" section={requestHeaders} />
        <PayloadSection
          title="req body"
          section={reqBodySection}
          action={
            <div className="inline-flex items-center gap-1">
              <BodyEyeButton
                ariaLabel="toggle req body"
                open={showReqBody}
                loading={loadingBody}
                onClick={toggleReqBody}
              />
              <BodyCopyButton
                ariaLabel={t("common.copy")}
                loading={loadingBody}
                onClick={() => void copyReqBody()}
              />
            </div>
          }
        />
        <PayloadSection title="resp headers" section={responseHeaders} />
        <PayloadSection
          title="resp body"
          section={respBodySection}
          action={
            <div className="inline-flex items-center gap-1">
              <BodyEyeButton
                ariaLabel="toggle resp body"
                open={showRespBody}
                loading={loadingBody}
                onClick={toggleRespBody}
              />
              <BodyCopyButton
                ariaLabel={t("common.copy")}
                loading={loadingBody}
                onClick={() => void copyRespBody()}
              />
            </div>
          }
        />
        {loadingBody ? <div className="text-xs text-muted">{t("common.loading")}</div> : null}
        {bodyError ? <div className="text-xs text-amber-700">{bodyError}</div> : null}
      </div>
    </details>
  );
}

export function RequestsModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [kind, setKind] = useState<RequestKind>("upstream");
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
  const [filters, setFilters] = useState({
    providerId: "",
    credentialId: "",
    userId: "",
    userKeyId: "",
    requestPathContains: "",
    fromAt: "",
    toAt: "",
    limit: "100"
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

  const filteredCredentialOptions = useMemo(() => {
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

  const filteredUserKeyOptions = useMemo(() => {
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

  const requestPathOptions = useMemo(() => {
    const dynamic = knownRequestPaths.filter((item) => item.trim().length > 0);
    const combined = Array.from(
      new Set<string>([...REQUEST_PATH_TEMPLATE_OPTIONS, ...dynamic])
    ).sort((a, b) => a.localeCompare(b));
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

  const buildSnapshot = (): RequestQuerySnapshot => {
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
  };

  const runQuery = () => {
    const snapshot = buildSnapshot();
    setActiveQuery(snapshot);
    setPage(1);
    setRows([]);
    setTotalRows(0);
    setBodyByTraceId({});
    setBodyLoadingByTraceId({});
    setBodyErrorByTraceId({});
  };

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

  const ensureBodyLoaded = async (row: RequestRow): Promise<RequestBodyPayload | undefined> => {
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
  };

  const tableColumns = [
    t("table.trace_id"),
    t("table.at"),
    t("table.status"),
    kind === "upstream" ? t("table.url") : t("table.path"),
    t("table.method"),
    t("table.payload")
  ];

  return (
    <Card title={t("requests.title")} subtitle={t("requests.subtitle")}>
      <div className="grid gap-3 md:grid-cols-3">
        <div>
          <Label>{t("field.kind")}</Label>
          <Select
            value={kind}
            onChange={(v) => setKind(v as RequestKind)}
            options={[
              { value: "upstream", label: t("requests.kind.upstream") },
              { value: "downstream", label: t("requests.kind.downstream") }
            ]}
          />
        </div>
        <div>
          <Label>{t("field.provider_id")}</Label>
          <Select
            value={filters.providerId}
            onChange={(v) => setFilters((p) => ({ ...p, providerId: v }))}
            options={providerOptions}
            disabled={kind !== "upstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.credential_id")}</Label>
          <Select
            value={filters.credentialId}
            onChange={(v) => setFilters((p) => ({ ...p, credentialId: v }))}
            options={filteredCredentialOptions}
            disabled={kind !== "upstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.user_id")}</Label>
          <SearchableSelect
            value={filters.userId}
            onChange={(v) => setFilters((p) => ({ ...p, userId: v }))}
            options={userOptions}
            placeholder={t("common.all")}
            noResultLabel={t("common.none")}
            disabled={kind !== "downstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.user_key_id")}</Label>
          <SearchableSelect
            value={filters.userKeyId}
            onChange={(v) => setFilters((p) => ({ ...p, userKeyId: v }))}
            options={filteredUserKeyOptions}
            placeholder={t("common.all")}
            noResultLabel={t("common.none")}
            disabled={kind !== "downstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.request_path_contains")}</Label>
          <SearchableSelect
            value={filters.requestPathContains}
            onChange={(v) => setFilters((p) => ({ ...p, requestPathContains: v }))}
            options={requestPathOptions}
            placeholder={t("requests.path.placeholder")}
            noResultLabel={t("common.none")}
          />
        </div>
        <div>
          <Label>{t("field.limit")}</Label>
          <Input value={filters.limit} onChange={(v) => setFilters((p) => ({ ...p, limit: v }))} />
        </div>
        <div>
          <Label>{t("field.from_at")}</Label>
          <Input
            value={filters.fromAt}
            onChange={(v) => setFilters((p) => ({ ...p, fromAt: v }))}
            placeholder={t("common.datetimePlaceholder")}
          />
        </div>
        <div>
          <Label>{t("field.to_at")}</Label>
          <Input
            value={filters.toAt}
            onChange={(v) => setFilters((p) => ({ ...p, toAt: v }))}
            placeholder={t("common.datetimePlaceholder")}
          />
        </div>
      </div>
      <div className="mt-3">
        <Button onClick={runQuery} disabled={loadingRows || loadingCount}>
          {loadingRows || loadingCount ? t("common.loading") : t("common.query")}
        </Button>
      </div>
      <div className="mt-4">
        <div className="query-result-table-wrap">
          <Table
            columns={tableColumns}
            rows={rows.map((row) => ({
              [tableColumns[0]]: row.trace_id,
              [tableColumns[1]]: formatAtForViewer(row.at),
              [tableColumns[2]]: row.response_status ?? "",
              [tableColumns[3]]:
                kind === "upstream"
                  ? ((row as UpstreamRequestQueryRow).request_url ?? "")
                  : (row as DownstreamRequestQueryRow).request_path,
              [tableColumns[4]]: row.request_method,
              [tableColumns[5]]: (
                <PayloadCell
                  row={row}
                  t={t}
                  notify={notify}
                  detail={bodyByTraceId[row.trace_id]}
                  loadingBody={Boolean(bodyLoadingByTraceId[row.trace_id])}
                  bodyError={bodyErrorByTraceId[row.trace_id]}
                  ensureBodyLoaded={ensureBodyLoaded}
                />
              )
            }))}
          />
        </div>
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
          <div>
            {t("common.pager.stats", {
              shown: rows.length,
              total: totalRows
            })}
          </div>
          <div className="flex items-center gap-2">
            <span>{t("common.show")}</span>
            <div className="w-20">
              <Select
                value={String(pageSize)}
                onChange={(value) => setPageSize(Number(value))}
                options={[
                  { value: "5", label: "5" },
                  { value: "10", label: "10" },
                  { value: "20", label: "20" },
                  { value: "50", label: "50" }
                ]}
              />
            </div>
            <Button
              variant="neutral"
              disabled={page <= 1 || loadingRows}
              onClick={() => setPage((prev) => Math.max(1, prev - 1))}
            >
              {t("common.pager.prev")}
            </Button>
            <span>{t("common.pager.page", { current: page, total: totalPages })}</span>
            <Button
              variant="neutral"
              disabled={page >= totalPages || loadingRows || loadingCount}
              onClick={() => setPage((prev) => Math.min(totalPages, prev + 1))}
            >
              {t("common.pager.next")}
            </Button>
          </div>
        </div>
      </div>
    </Card>
  );
}

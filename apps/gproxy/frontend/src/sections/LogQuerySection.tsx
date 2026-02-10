import { Fragment, useMemo, useState } from "react";

import { Button, Card, FieldLabel, TextInput } from "../components/ui";
import { useI18n } from "../i18n";
import { formatApiError, request } from "../lib/api";
import { beforeHoursRfc3339, formatDateTime, nowRfc3339, optional } from "../lib/format";
import type { LogQueryResponse, LogRecordKind } from "../lib/types";

type Props = {
  adminKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
};

type QueryKind = "all" | LogRecordKind;
type LogCursor = { at: string; id: number } | null;

function parseOptionalInt(input: string): number | undefined {
  const trimmed = input.trim();
  if (!trimmed) {
    return undefined;
  }
  const value = Number(trimmed);
  if (!Number.isInteger(value)) {
    throw new Error("invalid_integer");
  }
  return value;
}

export function LogQuerySection({ adminKey, notify }: Props) {
  const { t } = useI18n();
  const [kind, setKind] = useState<QueryKind>("all");
  const [from, setFrom] = useState(beforeHoursRfc3339(24));
  const [to, setTo] = useState(nowRfc3339());
  const [provider, setProvider] = useState("");
  const [credentialId, setCredentialId] = useState("");
  const [userId, setUserId] = useState("");
  const [userKeyId, setUserKeyId] = useState("");
  const [traceId, setTraceId] = useState("");
  const [operation, setOperation] = useState("");
  const [pathContains, setPathContains] = useState("");
  const [statusMin, setStatusMin] = useState("");
  const [statusMax, setStatusMax] = useState("");
  const [limit, setLimit] = useState("100");
  const [pageIndex, setPageIndex] = useState(0);
  const [cursorHistory, setCursorHistory] = useState<LogCursor[]>([null]);
  const [expandedRowKey, setExpandedRowKey] = useState<string | null>(null);
  const [data, setData] = useState<LogQueryResponse | null>(null);
  const [loading, setLoading] = useState(false);

  const rows = data?.rows ?? [];
  const hasMore = data?.has_more ?? false;

  const summaryText = useMemo(() => {
    const total = rows.length;
    if (total === 0) {
      return t("logs.range_info", { from: 0, to: 0, count: 0 });
    }
    const parsedLimit = Number(limit);
    const pageSize =
      data?.limit ?? (Number.isFinite(parsedLimit) && parsedLimit > 0 ? parsedLimit : 100);
    const fromValue = pageIndex * pageSize;
    const toValue = fromValue + total;
    return t("logs.range_info", { from: fromValue + 1, to: toValue, count: total });
  }, [data?.limit, limit, pageIndex, rows.length, t]);

  const runQuery = async (cursor: LogCursor = null) => {
    let parsedLimit = 100;
    let parsedCredentialId: number | undefined;
    let parsedUserId: number | undefined;
    let parsedUserKeyId: number | undefined;
    let parsedStatusMin: number | undefined;
    let parsedStatusMax: number | undefined;
    try {
      parsedLimit = parseOptionalInt(limit) ?? 100;
      if (parsedLimit < 1) {
        throw new Error("invalid_integer");
      }
      parsedCredentialId = parseOptionalInt(credentialId);
      parsedUserId = parseOptionalInt(userId);
      parsedUserKeyId = parseOptionalInt(userKeyId);
      parsedStatusMin = parseOptionalInt(statusMin);
      parsedStatusMax = parseOptionalInt(statusMax);
    } catch {
      notify("error", t("errors.invalid_number"));
      return;
    }

    setLoading(true);
    try {
      const result = await request<LogQueryResponse>("/admin/logs", {
        adminKey,
        query: {
          from,
          to,
          kind,
          provider: optional(provider),
          credential_id: parsedCredentialId,
          user_id: parsedUserId,
          user_key_id: parsedUserKeyId,
          trace_id: optional(traceId),
          operation: optional(operation),
          path_contains: optional(pathContains),
          status_min: parsedStatusMin,
          status_max: parsedStatusMax,
          limit: parsedLimit,
          cursor_at: cursor?.at,
          cursor_id: cursor?.id,
          include_body: true
        }
      });
      return result;
    } catch (error) {
      notify("error", formatApiError(error));
      return null;
    } finally {
      setLoading(false);
    }
  };

  const runFirstPage = async () => {
    const result = await runQuery(null);
    if (!result) {
      return;
    }
    setData(result);
    setPageIndex(0);
    setCursorHistory([null]);
    setExpandedRowKey(null);
  };

  const nextPage = () => {
    const nextCursorAt = data?.next_cursor_at;
    const nextCursorId = data?.next_cursor_id;
    if (!nextCursorAt || nextCursorId === null || nextCursorId === undefined) {
      return;
    }
    const nextCursor: LogCursor = { at: nextCursorAt, id: nextCursorId };
    void (async () => {
      const result = await runQuery(nextCursor);
      if (!result) {
        return;
      }
      setData(result);
      setCursorHistory((prev) => [...prev.slice(0, pageIndex + 1), nextCursor]);
      setPageIndex((prev) => prev + 1);
      setExpandedRowKey(null);
    })();
  };

  const prevPage = () => {
    if (pageIndex <= 0) {
      return;
    }
    const prevCursor = cursorHistory[pageIndex - 1] ?? null;
    void (async () => {
      const result = await runQuery(prevCursor);
      if (!result) {
        return;
      }
      setData(result);
      setPageIndex((prev) => Math.max(0, prev - 1));
      setExpandedRowKey(null);
    })();
  };

  return (
    <Card
      title={t("logs.title")}
      subtitle={t("logs.subtitle")}
      action={
        <div className="flex flex-wrap gap-2">
          <Button variant="neutral" onClick={() => {
            setKind("all");
            setFrom(beforeHoursRfc3339(24));
            setTo(nowRfc3339());
            setProvider("");
            setCredentialId("");
            setUserId("");
            setUserKeyId("");
            setTraceId("");
            setOperation("");
            setPathContains("");
            setStatusMin("");
            setStatusMax("");
            setLimit("100");
            setPageIndex(0);
            setCursorHistory([null]);
            setExpandedRowKey(null);
            setData(null);
          }}>
            {t("logs.reset")}
          </Button>
          <Button onClick={() => void runFirstPage()} disabled={loading}>
            {loading ? t("logs.querying") : t("logs.query")}
          </Button>
        </div>
      }
    >
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div>
          <FieldLabel>{t("logs.kind")}</FieldLabel>
          <select className="mt-2 select" value={kind} onChange={(event) => setKind(event.target.value as QueryKind)}>
            <option value="all">{t("logs.kind_all")}</option>
            <option value="upstream">{t("logs.kind_upstream")}</option>
            <option value="downstream">{t("logs.kind_downstream")}</option>
          </select>
        </div>
        <div>
          <FieldLabel>{t("logs.from")}</FieldLabel>
          <div className="mt-2">
            <TextInput value={from} onChange={setFrom} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.to")}</FieldLabel>
          <div className="mt-2">
            <TextInput value={to} onChange={setTo} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.limit")}</FieldLabel>
          <div className="mt-2">
            <TextInput type="number" value={limit} onChange={setLimit} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("common.provider")}</FieldLabel>
          <div className="mt-2">
            <TextInput value={provider} onChange={setProvider} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.credential_id")}</FieldLabel>
          <div className="mt-2">
            <TextInput type="number" value={credentialId} onChange={setCredentialId} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.user_id")}</FieldLabel>
          <div className="mt-2">
            <TextInput type="number" value={userId} onChange={setUserId} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.user_key_id")}</FieldLabel>
          <div className="mt-2">
            <TextInput type="number" value={userKeyId} onChange={setUserKeyId} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.trace_id")}</FieldLabel>
          <div className="mt-2">
            <TextInput value={traceId} onChange={setTraceId} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.operation")}</FieldLabel>
          <div className="mt-2">
            <TextInput value={operation} onChange={setOperation} />
          </div>
        </div>
        <div>
          <FieldLabel>{t("logs.path_contains")}</FieldLabel>
          <div className="mt-2">
            <TextInput value={pathContains} onChange={setPathContains} />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <FieldLabel>{t("logs.status_min")}</FieldLabel>
            <div className="mt-2">
              <TextInput type="number" value={statusMin} onChange={setStatusMin} />
            </div>
          </div>
          <div>
            <FieldLabel>{t("logs.status_max")}</FieldLabel>
            <div className="mt-2">
              <TextInput type="number" value={statusMax} onChange={setStatusMax} />
            </div>
          </div>
        </div>
      </div>

      <div className="mt-4 max-w-full overflow-x-auto rounded-xl border border-slate-200 bg-white">
        <table className="min-w-[1220px] text-left text-sm">
          <thead className="bg-slate-50 text-xs uppercase tracking-[0.08em] text-slate-600">
            <tr>
              <th className="px-3 py-2">{t("logs.col_time")}</th>
              <th className="px-3 py-2">{t("logs.col_kind")}</th>
              <th className="px-3 py-2">{t("common.provider")}</th>
              <th className="px-3 py-2">{t("logs.col_credential")}</th>
              <th className="px-3 py-2">{t("logs.col_user")}</th>
              <th className="px-3 py-2">{t("logs.col_user_key")}</th>
              <th className="px-3 py-2">{t("logs.col_attempt")}</th>
              <th className="px-3 py-2">{t("logs.col_operation")}</th>
              <th className="px-3 py-2">{t("logs.col_method")}</th>
              <th className="px-3 py-2">{t("logs.col_path")}</th>
              <th className="px-3 py-2">{t("logs.col_status")}</th>
              <th className="px-3 py-2">{t("logs.col_trace")}</th>
              <th className="px-3 py-2">{t("logs.col_error")}</th>
              <th className="px-3 py-2">{t("logs.col_detail")}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {rows.length === 0 ? (
              <tr>
                <td colSpan={14} className="px-4 py-8 text-center text-sm text-slate-500">
                  {loading ? t("common.loading") : t("logs.empty")}
                </td>
              </tr>
            ) : (
              rows.map((row) => {
                const rowKey = `${row.kind}-${row.id}`;
                const expanded = rowKey === expandedRowKey;
                const errorText = [row.error_kind, row.error_message].filter(Boolean).join(": ");
                return (
                  <Fragment key={rowKey}>
                    <tr className="align-top hover:bg-slate-50/80">
                      <td className="px-3 py-2 whitespace-nowrap">{formatDateTime(row.at)}</td>
                      <td className="px-3 py-2 whitespace-nowrap">
                        <span className={`badge ${row.kind === "upstream" ? "badge-active" : ""}`}>
                          {row.kind === "upstream" ? t("logs.kind_upstream") : t("logs.kind_downstream")}
                        </span>
                      </td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.provider ?? "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.credential_id ?? "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.user_id ?? "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.user_key_id ?? "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.attempt_no ?? "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.operation ?? "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.request_method}</td>
                      <td className="px-3 py-2 font-mono text-xs">{row.request_path}</td>
                      <td className="px-3 py-2 whitespace-nowrap">{row.response_status ?? "-"}</td>
                      <td className="px-3 py-2 font-mono text-xs">{row.trace_id ?? "-"}</td>
                      <td className="px-3 py-2">{errorText || "-"}</td>
                      <td className="px-3 py-2 whitespace-nowrap">
                        <Button
                          variant="neutral"
                          onClick={() => setExpandedRowKey(expanded ? null : rowKey)}
                        >
                          {expanded ? t("logs.collapse") : t("logs.expand")}
                        </Button>
                      </td>
                    </tr>
                    {expanded ? (
                      <tr className="bg-slate-50/70">
                        <td colSpan={14} className="px-3 py-3">
                          <div className="grid gap-3 lg:grid-cols-2">
                            <div className="rounded-lg border border-slate-200 bg-white p-2">
                              <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                                {t("logs.request_body")}
                              </div>
                              {row.request_body ? (
                                <pre className="max-h-[380px] overflow-auto whitespace-pre-wrap break-all rounded-md bg-slate-950 p-2 text-xs text-emerald-100">
                                  {row.request_body}
                                </pre>
                              ) : (
                                <div className="text-xs text-slate-400">{t("logs.no_body")}</div>
                              )}
                            </div>
                            <div className="rounded-lg border border-slate-200 bg-white p-2">
                              <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-slate-500">
                                {t("logs.response_body")}
                              </div>
                              {row.response_body ? (
                                <pre className="max-h-[380px] overflow-auto whitespace-pre-wrap break-all rounded-md bg-slate-950 p-2 text-xs text-emerald-100">
                                  {row.response_body}
                                </pre>
                              ) : (
                                <div className="text-xs text-slate-400">{t("logs.no_body")}</div>
                              )}
                            </div>
                          </div>
                        </td>
                      </tr>
                    ) : null}
                  </Fragment>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      <div className="mt-4 flex flex-wrap items-center justify-between gap-2">
        <div className="text-sm text-slate-500">{summaryText}</div>
        <div className="flex items-center gap-2">
          <Button variant="neutral" onClick={prevPage} disabled={loading || pageIndex <= 0}>
            {t("logs.prev")}
          </Button>
          <Button variant="neutral" onClick={nextPage} disabled={loading || !hasMore}>
            {t("logs.next")}
          </Button>
        </div>
      </div>
    </Card>
  );
}

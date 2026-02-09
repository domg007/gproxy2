import { useMemo, useState } from "react";

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
  const [offset, setOffset] = useState(0);
  const [data, setData] = useState<LogQueryResponse | null>(null);
  const [loading, setLoading] = useState(false);

  const rows = data?.rows ?? [];
  const hasMore = data?.has_more ?? false;

  const summaryText = useMemo(() => {
    const total = rows.length;
    if (total === 0) {
      return t("logs.range_info", { from: 0, to: 0, count: 0 });
    }
    const fromValue = data?.offset ?? offset;
    const toValue = fromValue + total;
    return t("logs.range_info", { from: fromValue + 1, to: toValue, count: total });
  }, [data?.offset, offset, rows.length, t]);

  const runQuery = async (nextOffset = 0) => {
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
          offset: nextOffset
        }
      });
      setData(result);
      setOffset(result.offset);
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setLoading(false);
    }
  };

  const nextPage = () => {
    const fallback = Number(limit);
    const size = data?.limit ?? (Number.isFinite(fallback) && fallback > 0 ? fallback : 100);
    const nextOffset = (data?.offset ?? offset) + size;
    void runQuery(nextOffset);
  };

  const prevPage = () => {
    const fallback = Number(limit);
    const size = data?.limit ?? (Number.isFinite(fallback) && fallback > 0 ? fallback : 100);
    const nextOffset = Math.max(0, (data?.offset ?? offset) - size);
    void runQuery(nextOffset);
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
            setOffset(0);
            setData(null);
          }}>
            {t("logs.reset")}
          </Button>
          <Button onClick={() => void runQuery(0)} disabled={loading}>
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

      <div className="mt-4 overflow-x-auto rounded-xl border border-slate-200 bg-white">
        <table className="min-w-[1450px] text-left text-sm">
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
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {rows.length === 0 ? (
              <tr>
                <td colSpan={13} className="px-4 py-8 text-center text-sm text-slate-500">
                  {loading ? t("common.loading") : t("logs.empty")}
                </td>
              </tr>
            ) : (
              rows.map((row) => {
                const errorText = [row.error_kind, row.error_message].filter(Boolean).join(": ");
                return (
                  <tr key={`${row.kind}-${row.id}`} className="align-top hover:bg-slate-50/80">
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
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      <div className="mt-4 flex flex-wrap items-center justify-between gap-2">
        <div className="text-sm text-slate-500">{summaryText}</div>
        <div className="flex items-center gap-2">
          <Button variant="neutral" onClick={prevPage} disabled={loading || (data?.offset ?? 0) <= 0}>
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

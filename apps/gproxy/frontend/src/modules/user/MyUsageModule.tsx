import { useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type { UsageQueryRow, UsageSummary } from "../../lib/types";
import { Button, Card, Input, Label, MetricCard, Table } from "../../components/ui";

function emptySummary(): UsageSummary {
  return {
    count: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_input_tokens: 0,
    cache_creation_input_tokens: 0
  };
}

export function MyUsageModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState<UsageQueryRow[]>([]);
  const [summary, setSummary] = useState<UsageSummary>(emptySummary());
  const [filters, setFilters] = useState({
    channel: "",
    model: "",
    userKeyId: "",
    fromAt: "",
    toAt: "",
    limit: "200"
  });

  const payload = () => {
    const userKeyId = parseOptionalI64(filters.userKeyId);
    const fromUnixMs = parseDateTimeLocalToUnixMs(filters.fromAt);
    const toUnixMs = parseDateTimeLocalToUnixMs(filters.toAt);
    const limit = parseOptionalI64(filters.limit) ?? 200;

    return {
      channel: filters.channel.trim() ? scopeEq(filters.channel.trim()) : scopeAll<string>(),
      model: filters.model.trim() ? scopeEq(filters.model.trim()) : scopeAll<string>(),
      user_id: scopeAll<number>(),
      user_key_id: userKeyId === null ? scopeAll<number>() : scopeEq(userKeyId),
      from_unix_ms: fromUnixMs,
      to_unix_ms: toUnixMs,
      limit
    };
  };

  const query = async () => {
    try {
      const [queryResult, summaryResult] = await Promise.all([
        apiRequest<UsageQueryRow[]>("/user/usages/query", {
          apiKey,
          method: "POST",
          body: payload()
        }),
        apiRequest<UsageSummary>("/user/usages/summary", {
          apiKey,
          method: "POST",
          body: payload()
        })
      ]);
      setRows(queryResult);
      setSummary(summaryResult);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const tableColumns = [
    t("table.trace_id"),
    t("table.provider"),
    t("table.model"),
    t("table.input"),
    t("table.output"),
    t("table.at")
  ];

  return (
    <div className="space-y-4">
      <Card title={t("myUsage.title")} subtitle={t("myUsage.subtitle")}>
        <div className="grid gap-3 md:grid-cols-3">
          <div>
            <Label>{t("field.channel")}</Label>
            <Input value={filters.channel} onChange={(v) => setFilters((p) => ({ ...p, channel: v }))} />
          </div>
          <div>
            <Label>{t("field.model")}</Label>
            <Input value={filters.model} onChange={(v) => setFilters((p) => ({ ...p, model: v }))} />
          </div>
          <div>
            <Label>{t("field.user_key_id")}</Label>
            <Input
              value={filters.userKeyId}
              onChange={(v) => setFilters((p) => ({ ...p, userKeyId: v }))}
            />
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
          <div>
            <Label>{t("field.limit")}</Label>
            <Input value={filters.limit} onChange={(v) => setFilters((p) => ({ ...p, limit: v }))} />
          </div>
        </div>
        <div className="mt-3">
          <Button onClick={() => void query()}>{t("common.query")}</Button>
        </div>
      </Card>
      <div className="grid gap-3 md:grid-cols-5">
        <MetricCard label={t("metric.count")} value={summary.count} />
        <MetricCard label={t("metric.input_tokens")} value={summary.input_tokens} />
        <MetricCard label={t("metric.output_tokens")} value={summary.output_tokens} />
        <MetricCard label={t("metric.cache_read")} value={summary.cache_read_input_tokens} />
        <MetricCard label={t("metric.cache_creation")} value={summary.cache_creation_input_tokens} />
      </div>
      <Card title={t("myUsage.rows")}>
        <Table
          columns={tableColumns}
          rows={rows.map((row) => ({
            [tableColumns[0]]: row.trace_id,
            [tableColumns[1]]: row.provider_channel ?? "",
            [tableColumns[2]]: row.model ?? "",
            [tableColumns[3]]: row.input_tokens ?? "",
            [tableColumns[4]]: row.output_tokens ?? "",
            [tableColumns[5]]: formatAtForViewer(row.at)
          }))}
        />
      </Card>
    </div>
  );
}

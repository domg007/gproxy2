import { useEffect, useMemo, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type { UsageQueryCount, UsageQueryRow, UsageSummary, UserKeyQueryRow } from "../../lib/types";
import { Button, Card, Input, Label, MetricCard, SearchableSelect, Select, Table } from "../../components/ui";

type UsageQuerySnapshot = {
  channel: string;
  model: string;
  userKeyId: number | null;
  fromUnixMs: number | null;
  toUnixMs: number | null;
  maxRows: number | null;
};

function emptySummary(): UsageSummary {
  return {
    count: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_input_tokens: 0,
    cache_creation_input_tokens: 0,
    cache_creation_input_tokens_5min: 0,
    cache_creation_input_tokens_1h: 0
  };
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

function toPositiveOrNull(value: number | null): number | null {
  if (value === null || value <= 0) {
    return null;
  }
  return value;
}

function buildUsageBasePayload(snapshot: UsageQuerySnapshot) {
  return {
    channel: snapshot.channel ? scopeEq(snapshot.channel) : scopeAll<string>(),
    model: snapshot.model ? scopeEq(snapshot.model) : scopeAll<string>(),
    user_id: scopeAll<number>(),
    user_key_id: snapshot.userKeyId === null ? scopeAll<number>() : scopeEq(snapshot.userKeyId),
    from_unix_ms: snapshot.fromUnixMs,
    to_unix_ms: snapshot.toUnixMs
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
  const [totalRows, setTotalRows] = useState(0);
  const [pageSize, setPageSize] = useState<number>(() => defaultPageSizeByViewport());
  const [page, setPage] = useState(1);
  const [activeQuery, setActiveQuery] = useState<UsageQuerySnapshot | null>(null);
  const [loadingRows, setLoadingRows] = useState(false);
  const [loadingMeta, setLoadingMeta] = useState(false);
  const [userKeyRows, setUserKeyRows] = useState<UserKeyQueryRow[]>([]);
  const [knownChannels, setKnownChannels] = useState<string[]>([]);
  const [knownModels, setKnownModels] = useState<string[]>([]);
  const [knownModelsByChannel, setKnownModelsByChannel] = useState<Record<string, string[]>>({});
  const [filters, setFilters] = useState({
    channel: "",
    model: "",
    userKeyId: "",
    fromAt: "",
    toAt: "",
    limit: "200"
  });

  const selectedChannel = filters.channel.trim();

  const loadUserKeys = async () => {
    try {
      const data = await apiRequest<UserKeyQueryRow[]>("/user/keys/query", {
        apiKey,
        method: "POST"
      });
      setUserKeyRows([...data].sort((a, b) => a.id - b.id));
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const collectUsageMetadata = (usageRows: UsageQueryRow[]) => {
    const channels = usageRows
      .map((row) => row.provider_channel?.trim() ?? "")
      .filter((value) => value.length > 0);
    const models = usageRows
      .map((row) => row.model?.trim() ?? "")
      .filter((value) => value.length > 0);
    const channelModelPairs = usageRows
      .map((row) => ({
        channel: row.provider_channel?.trim() ?? "",
        model: row.model?.trim() ?? ""
      }))
      .filter((item) => item.channel.length > 0 && item.model.length > 0);
    if (channels.length > 0) {
      setKnownChannels((prev) => Array.from(new Set([...prev, ...channels])).sort());
    }
    if (models.length > 0) {
      setKnownModels((prev) => Array.from(new Set([...prev, ...models])).sort());
    }
    if (channelModelPairs.length > 0) {
      setKnownModelsByChannel((prev) => {
        const merged = new Map<string, Set<string>>();
        for (const [channel, channelModels] of Object.entries(prev)) {
          merged.set(channel, new Set(channelModels));
        }
        for (const item of channelModelPairs) {
          const models = merged.get(item.channel);
          if (models) {
            models.add(item.model);
          } else {
            merged.set(item.channel, new Set([item.model]));
          }
        }
        const next: Record<string, string[]> = {};
        for (const [channel, modelSet] of merged.entries()) {
          next[channel] = Array.from(modelSet).sort();
        }
        return next;
      });
    }
  };

  const loadUsageFilterOptions = async () => {
    try {
      const data = await apiRequest<UsageQueryRow[]>("/user/usages/query", {
        apiKey,
        method: "POST",
        body: {
          channel: scopeAll<string>(),
          model: scopeAll<string>(),
          user_id: scopeAll<number>(),
          user_key_id: scopeAll<number>(),
          from_unix_ms: null,
          to_unix_ms: null,
          offset: 0,
          limit: 1000
        }
      });
      collectUsageMetadata(data);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  useEffect(() => {
    void loadUserKeys();
    void loadUsageFilterOptions();
  }, [apiKey]);

  const buildSnapshot = (): UsageQuerySnapshot => {
    const userKeyId = parseOptionalI64(filters.userKeyId);
    const fromUnixMs = parseDateTimeLocalToUnixMs(filters.fromAt);
    const toUnixMs = parseDateTimeLocalToUnixMs(filters.toAt);
    return {
      channel: filters.channel.trim(),
      model: filters.model.trim(),
      userKeyId,
      fromUnixMs,
      toUnixMs,
      maxRows: toPositiveOrNull(parseOptionalI64(filters.limit))
    };
  };

  const query = () => {
    const snapshot = buildSnapshot();
    setActiveQuery(snapshot);
    setPage(1);
    setRows([]);
    setTotalRows(0);
  };

  const tableColumns = [
    t("table.trace_id"),
    t("table.provider"),
    t("table.model"),
    t("table.input"),
    t("table.output"),
    t("table.cache_read"),
    t("table.cache_creation"),
    t("table.cache_creation_5m"),
    t("table.cache_creation_1h"),
    t("table.at")
  ];

  const userKeyOptions = useMemo(
    () => [
      { value: "", label: t("common.all") },
      ...userKeyRows.map((row) => ({
        value: String(row.id),
        label: `#${row.id} · ${row.api_key}`
      }))
    ],
    [t, userKeyRows]
  );

  const channelOptions = useMemo(
    () => [
      { value: "", label: t("common.all") },
      ...knownChannels.map((value) => ({ value, label: value }))
    ],
    [knownChannels, t]
  );

  const modelOptions = useMemo(
    () => {
      const scopedModels =
        selectedChannel.length > 0 ? (knownModelsByChannel[selectedChannel] ?? []) : knownModels;
      return [
        { value: "", label: t("common.all") },
        ...scopedModels.map((value) => ({ value, label: value }))
      ];
    },
    [knownModels, knownModelsByChannel, selectedChannel, t]
  );

  useEffect(() => {
    if (!filters.model) {
      return;
    }
    const exists = modelOptions.some((item) => item.value === filters.model);
    if (!exists) {
      setFilters((prev) => ({ ...prev, model: "" }));
    }
  }, [filters.model, modelOptions]);

  useEffect(() => {
    setPage(1);
  }, [pageSize]);

  const totalPages = Math.max(1, Math.ceil(totalRows / pageSize));

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  useEffect(() => {
    if (!activeQuery) {
      return;
    }
    setLoadingMeta(true);
    const fetchMeta = async () => {
      try {
        const basePayload = buildUsageBasePayload(activeQuery);
        const [countResult, summaryResult] = await Promise.all([
          apiRequest<UsageQueryCount>("/user/usages/count", {
            apiKey,
            method: "POST",
            body: basePayload
          }),
          apiRequest<UsageSummary>("/user/usages/summary", {
            apiKey,
            method: "POST",
            body: basePayload
          })
        ]);
        const maxRows = activeQuery.maxRows;
        setTotalRows(maxRows === null ? countResult.count : Math.min(countResult.count, maxRows));
        setSummary(summaryResult);
      } catch (error) {
        notify("error", formatError(error));
      } finally {
        setLoadingMeta(false);
      }
    };
    void fetchMeta();
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
    setLoadingRows(true);
    const fetchRows = async () => {
      try {
        const data = await apiRequest<UsageQueryRow[]>("/user/usages/query", {
          apiKey,
          method: "POST",
          body: {
            ...buildUsageBasePayload(activeQuery),
            offset,
            limit
          }
        });
        setRows(data);
        collectUsageMetadata(data);
      } catch (error) {
        notify("error", formatError(error));
      } finally {
        setLoadingRows(false);
      }
    };
    void fetchRows();
  }, [activeQuery, apiKey, notify, page, pageSize]);

  return (
    <div className="space-y-4">
      <Card title={t("myUsage.title")} subtitle={t("myUsage.subtitle")}>
        <div className="grid gap-3 md:grid-cols-3">
          <div>
            <Label>{t("field.channel")}</Label>
            <SearchableSelect
              value={filters.channel}
              onChange={(v) => setFilters((p) => ({ ...p, channel: v }))}
              options={channelOptions}
              placeholder={t("common.all")}
              noResultLabel={t("common.none")}
            />
          </div>
          <div>
            <Label>{t("field.model")}</Label>
            <SearchableSelect
              value={filters.model}
              onChange={(v) => setFilters((p) => ({ ...p, model: v }))}
              options={modelOptions}
              placeholder={t("common.all")}
              noResultLabel={t("common.none")}
            />
          </div>
          <div>
            <Label>{t("field.user_key_id")}</Label>
            <SearchableSelect
              value={filters.userKeyId}
              onChange={(v) => setFilters((p) => ({ ...p, userKeyId: v }))}
              options={userKeyOptions}
              placeholder={t("common.all")}
              noResultLabel={t("common.none")}
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
          <Button onClick={query} disabled={loadingRows || loadingMeta}>
            {loadingRows || loadingMeta ? t("common.loading") : t("common.query")}
          </Button>
        </div>
      </Card>
      <div className="grid gap-3 md:grid-cols-7">
        <MetricCard label={t("metric.count")} value={summary.count} />
        <MetricCard label={t("metric.input_tokens")} value={summary.input_tokens} />
        <MetricCard label={t("metric.output_tokens")} value={summary.output_tokens} />
        <MetricCard label={t("metric.cache_read")} value={summary.cache_read_input_tokens} />
        <MetricCard label={t("metric.cache_creation")} value={summary.cache_creation_input_tokens} />
        <MetricCard label={t("metric.cache_creation_5m")} value={summary.cache_creation_input_tokens_5min} />
        <MetricCard label={t("metric.cache_creation_1h")} value={summary.cache_creation_input_tokens_1h} />
      </div>
      <Card title={t("myUsage.rows")}>
        <div className="query-result-table-wrap">
          <Table
            columns={tableColumns}
            rows={rows.map((row) => ({
              [tableColumns[0]]: row.downstream_trace_id ?? row.trace_id,
              [tableColumns[1]]: row.provider_channel ?? "",
              [tableColumns[2]]: row.model ?? "",
              [tableColumns[3]]: row.input_tokens ?? "",
              [tableColumns[4]]: row.output_tokens ?? "",
              [tableColumns[5]]: row.cache_read_input_tokens ?? "",
              [tableColumns[6]]: row.cache_creation_input_tokens ?? "",
              [tableColumns[7]]: row.cache_creation_input_tokens_5min ?? "",
              [tableColumns[8]]: row.cache_creation_input_tokens_1h ?? "",
              [tableColumns[9]]: formatAtForViewer(row.at)
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
              disabled={page >= totalPages || loadingRows || loadingMeta}
              onClick={() => setPage((prev) => Math.min(totalPages, prev + 1))}
            >
              {t("common.pager.next")}
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}

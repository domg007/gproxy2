import { useEffect, useMemo, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type { UsageQueryRow, UsageSummary } from "../../lib/types";
import { Button, Card, Input, Label, MetricCard, SearchableSelect, Select, Table } from "../../components/ui";
import { useAdminFilterOptions } from "./hooks/useAdminFilterOptions";

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

export function UsageModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState<UsageQueryRow[]>([]);
  const [pageSize, setPageSize] = useState<number>(() => defaultPageSizeByViewport());
  const [page, setPage] = useState(1);
  const [summary, setSummary] = useState<UsageSummary>(emptySummary());
  const [knownChannels, setKnownChannels] = useState<string[]>([]);
  const [knownModels, setKnownModels] = useState<string[]>([]);
  const [knownModelsByChannel, setKnownModelsByChannel] = useState<Record<string, string[]>>({});
  const {
    isLoading: isFilterOptionsLoading,
    providerRows,
    userRows,
    userKeyRows,
    userOptions
  } = useAdminFilterOptions({
    apiKey,
    notify,
    t
  });
  const [filters, setFilters] = useState({
    channel: "",
    model: "",
    userId: "",
    userKeyId: "",
    fromAt: "",
    toAt: "",
    limit: "200"
  });

  const selectedChannel = filters.channel.trim();

  const selectedUserId = useMemo(() => {
    const value = Number(filters.userId);
    return Number.isInteger(value) ? value : null;
  }, [filters.userId]);

  const buildPayload = () => {
    const userId = parseOptionalI64(filters.userId);
    const userKeyId = parseOptionalI64(filters.userKeyId);
    const fromUnixMs = parseDateTimeLocalToUnixMs(filters.fromAt);
    const toUnixMs = parseDateTimeLocalToUnixMs(filters.toAt);
    const limit = parseOptionalI64(filters.limit) ?? 200;

    return {
      channel: filters.channel.trim() ? scopeEq(filters.channel.trim()) : scopeAll<string>(),
      model: filters.model.trim() ? scopeEq(filters.model.trim()) : scopeAll<string>(),
      user_id: userId === null ? scopeAll<number>() : scopeEq(userId),
      user_key_id: userKeyId === null ? scopeAll<number>() : scopeEq(userKeyId),
      from_unix_ms: fromUnixMs,
      to_unix_ms: toUnixMs,
      limit
    };
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
      const data = await apiRequest<UsageQueryRow[]>("/admin/usages/query", {
        apiKey,
        method: "POST",
        body: {
          channel: scopeAll<string>(),
          model: scopeAll<string>(),
          user_id: scopeAll<number>(),
          user_key_id: scopeAll<number>(),
          from_unix_ms: null,
          to_unix_ms: null,
          limit: 1000
        }
      });
      collectUsageMetadata(data);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  useEffect(() => {
    void loadUsageFilterOptions();
  }, [apiKey]);

  const query = async () => {
    try {
      const [rowsResult, summaryResult] = await Promise.all([
        apiRequest<UsageQueryRow[]>("/admin/usages/query", {
          apiKey,
          method: "POST",
          body: buildPayload()
        }),
        apiRequest<UsageSummary>("/admin/usages/summary", {
          apiKey,
          method: "POST",
          body: buildPayload()
        })
      ]);
      setRows(rowsResult);
      setPage(1);
      setSummary(summaryResult);
      collectUsageMetadata(rowsResult);
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
    t("table.cache_read"),
    t("table.cache_creation"),
    t("table.cache_creation_5m"),
    t("table.cache_creation_1h"),
    t("table.at")
  ];

  const channelOptions = useMemo(() => {
    const channels = Array.from(
      new Set([
        ...providerRows.map((row) => row.channel.trim()).filter((value) => value.length > 0),
        ...knownChannels
      ])
    ).sort();
    return [
      { value: "", label: t("common.all") },
      ...channels.map((value) => ({ value, label: value }))
    ];
  }, [knownChannels, providerRows, t]);

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

  const userById = useMemo(() => new Map(userRows.map((row) => [row.id, row])), [userRows]);

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
        const preview =
          key.length <= 14 ? key : `${key.slice(0, 6)}...${key.slice(-4)}`;
        return {
          value: String(row.id),
          label: `#${row.id} · ${userMeta} · ${preview}`
        };
      })
    ];
  }, [selectedUserId, t, userById, userKeyRows]);

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

  const totalPages = Math.max(1, Math.ceil(rows.length / pageSize));

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  const pagedRows = useMemo(() => {
    const start = (page - 1) * pageSize;
    return rows.slice(start, start + pageSize);
  }, [page, pageSize, rows]);

  return (
    <div className="space-y-4">
      <Card title={t("usage.title")} subtitle={t("usage.subtitle")}>
        <div className="grid gap-3 md:grid-cols-3">
          <div>
            <Label>{t("field.channel")}</Label>
            <SearchableSelect
              value={filters.channel}
              onChange={(v) => setFilters((p) => ({ ...p, channel: v }))}
              options={channelOptions}
              placeholder={t("common.all")}
              noResultLabel={t("common.none")}
              disabled={isFilterOptionsLoading}
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
              disabled={isFilterOptionsLoading}
            />
          </div>
          <div>
            <Label>{t("field.user_id")}</Label>
            <Select
              value={filters.userId}
              onChange={(v) => setFilters((p) => ({ ...p, userId: v }))}
              options={userOptions}
              disabled={isFilterOptionsLoading}
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
              disabled={isFilterOptionsLoading}
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
      <div className="grid gap-3 md:grid-cols-7">
        <MetricCard label={t("metric.count")} value={summary.count} />
        <MetricCard label={t("metric.input_tokens")} value={summary.input_tokens} />
        <MetricCard label={t("metric.output_tokens")} value={summary.output_tokens} />
        <MetricCard label={t("metric.cache_read")} value={summary.cache_read_input_tokens} />
        <MetricCard label={t("metric.cache_creation")} value={summary.cache_creation_input_tokens} />
        <MetricCard label={t("metric.cache_creation_5m")} value={summary.cache_creation_input_tokens_5min} />
        <MetricCard label={t("metric.cache_creation_1h")} value={summary.cache_creation_input_tokens_1h} />
      </div>
      <Card title={t("usage.rows")}>
        <Table
          columns={tableColumns}
          rows={pagedRows.map((row) => ({
            [tableColumns[0]]: row.trace_id,
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
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
          <div>
            {t("common.pager.stats", {
              shown: pagedRows.length,
              total: rows.length
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
              disabled={page <= 1}
              onClick={() => setPage((prev) => Math.max(1, prev - 1))}
            >
              {t("common.pager.prev")}
            </Button>
            <span>{t("common.pager.page", { current: page, total: totalPages })}</span>
            <Button
              variant="neutral"
              disabled={page >= totalPages}
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

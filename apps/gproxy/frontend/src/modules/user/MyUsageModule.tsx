import { useEffect, useMemo, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type { UsageQueryRow, UsageSummary, UserKeyQueryRow } from "../../lib/types";
import { Button, Card, Input, Label, MetricCard, SearchableSelect, Table } from "../../components/ui";

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
      collectUsageMetadata(queryResult);
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

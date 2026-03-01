import { useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type { DownstreamRequestQueryRow, UpstreamRequestQueryRow } from "../../lib/types";
import { Button, Card, Input, Label, SearchableSelect, Select, Table } from "../../components/ui";
import { useAdminFilterOptions } from "./hooks/useAdminFilterOptions";

export function RequestsModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [kind, setKind] = useState<"upstream" | "downstream">("upstream");
  const [rows, setRows] = useState<Array<UpstreamRequestQueryRow | DownstreamRequestQueryRow>>([]);
  const {
    isLoading: isFilterOptionsLoading,
    providerOptions,
    credentialOptions,
    userOptions,
    userKeyOptions
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
    fromAt: "",
    toAt: "",
    limit: "100"
  });

  const query = async () => {
    try {
      const providerId = parseOptionalI64(filters.providerId);
      const credentialId = parseOptionalI64(filters.credentialId);
      const userId = parseOptionalI64(filters.userId);
      const userKeyId = parseOptionalI64(filters.userKeyId);
      const fromUnixMs = parseDateTimeLocalToUnixMs(filters.fromAt);
      const toUnixMs = parseDateTimeLocalToUnixMs(filters.toAt);
      const limit = parseOptionalI64(filters.limit) ?? 100;

      if (kind === "upstream") {
        const data = await apiRequest<UpstreamRequestQueryRow[]>("/admin/requests/upstream/query", {
          apiKey,
          method: "POST",
          body: {
            provider_id: providerId === null ? scopeAll<number>() : scopeEq(providerId),
            credential_id: credentialId === null ? scopeAll<number>() : scopeEq(credentialId),
            from_unix_ms: fromUnixMs,
            to_unix_ms: toUnixMs,
            limit
          }
        });
        setRows(data);
        return;
      }

      const data = await apiRequest<DownstreamRequestQueryRow[]>("/admin/requests/downstream/query", {
        apiKey,
        method: "POST",
        body: {
          user_id: userId === null ? scopeAll<number>() : scopeEq(userId),
          user_key_id: userKeyId === null ? scopeAll<number>() : scopeEq(userKeyId),
          from_unix_ms: fromUnixMs,
          to_unix_ms: toUnixMs,
          limit
        }
      });
      setRows(data);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const tableColumns = [
    t("table.trace_id"),
    t("table.at"),
    t("table.status"),
    kind === "upstream" ? t("table.url") : t("table.path")
  ];

  return (
    <Card title={t("requests.title")} subtitle={t("requests.subtitle")}>
      <div className="grid gap-3 md:grid-cols-3">
        <div>
          <Label>{t("field.kind")}</Label>
          <Select
            value={kind}
            onChange={(v) => setKind(v as "upstream" | "downstream")}
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
            options={credentialOptions}
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
            options={userKeyOptions}
            placeholder={t("common.all")}
            noResultLabel={t("common.none")}
            disabled={kind !== "downstream" || isFilterOptionsLoading}
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
        <Button onClick={() => void query()}>{t("common.query")}</Button>
      </div>
      <div className="mt-4">
        <Table
          columns={tableColumns}
          rows={rows.map((row) => ({
            [tableColumns[0]]: row.trace_id,
            [tableColumns[1]]: formatAtForViewer(row.at),
            [tableColumns[2]]: row.response_status ?? "",
            [tableColumns[3]]:
              kind === "upstream"
                ? ((row as UpstreamRequestQueryRow).request_url ?? "")
                : (row as DownstreamRequestQueryRow).request_path
          }))}
        />
      </div>
    </Card>
  );
}

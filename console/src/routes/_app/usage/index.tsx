import { useCallback, useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { usageQuery, auditQuery, type Usage, type AuditLog, type UsageFilter } from "@/api/usage";
import { providersQuery } from "@/api/providers";
import { DataTable, type DataColumn } from "@/components/data-table";
import { UsageFilters } from "@/components/observability/usage-filters";
import { RequestDrawer } from "@/components/observability/request-drawer";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

const PAGE = 50;

export const Route = createFileRoute("/_app/usage/")({
  loader: ({ context }) => {
    void context.queryClient.ensureQueryData(providersQuery);
  },
  component: UsagePage,
});

function fmtAt(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function UsagePage() {
  const { t } = useTranslation("observability");
  const queryClient = useQueryClient();

  // Filters (without pagination cursors)
  const [filter, setFilter] = useState<Omit<UsageFilter, "before_id" | "limit">>({});
  // Accumulated rows for the explorer
  const [rows, setRows] = useState<Usage[]>([]);
  // Whether we know there are more rows
  const [hasMore, setHasMore] = useState(true);
  // Loading more state
  const [loadingMore, setLoadingMore] = useState(false);
  // Drawer state
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [selectedRid, setSelectedRid] = useState<string | null>(null);

  const { data: providers } = useQuery(providersQuery);
  const providerMap = new Map((providers ?? []).map((p) => [p.id, p.label ?? p.name]));

  // First-page query: reset rows whenever filter changes
  const firstPageQuery = usageQuery({ ...filter, limit: PAGE });
  const { data: firstPage, isPending } = useQuery(firstPageQuery);

  // When first page data arrives (or filter changes), reset accumulated rows
  useEffect(() => {
    if (firstPage !== undefined) {
      setRows(firstPage);
      setHasMore(firstPage.length >= PAGE);
    }
  }, [firstPage]);

  // Reset everything when filter changes
  const handleFilterChange = useCallback(
    (newFilter: Omit<UsageFilter, "before_id" | "limit">) => {
      setFilter(newFilter);
      setRows([]);
      setHasMore(true);
    },
    [],
  );

  const loadMore = useCallback(async () => {
    if (loadingMore || rows.length === 0) return;
    const lastId = rows.at(-1)?.id;
    if (lastId == null) return;
    setLoadingMore(true);
    try {
      const nextPage = await queryClient.fetchQuery(
        usageQuery({ ...filter, before_id: lastId, limit: PAGE }),
      );
      setRows((prev) => [...prev, ...nextPage]);
      setHasMore(nextPage.length >= PAGE);
    } finally {
      setLoadingMore(false);
    }
  }, [filter, rows, loadingMore, queryClient]);

  function openDrawer(row: Usage) {
    setSelectedRid(row.request_id);
    setDrawerOpen(true);
  }

  // Usage DataTable columns
  const usageCols: DataColumn<Usage>[] = [
    {
      key: "at",
      header: t("usage.columns.at"),
      cell: (r) => (
        <span className="whitespace-nowrap font-mono text-xs text-muted-foreground">
          {fmtAt(r.at)}
        </span>
      ),
    },
    {
      key: "operation",
      header: t("usage.columns.operation"),
      cell: (r) => (
        <span className="font-mono text-xs">
          {r.operation}
          {r.kind && r.kind !== r.operation && (
            <span className="text-muted-foreground"> / {r.kind}</span>
          )}
        </span>
      ),
    },
    {
      key: "model",
      header: t("usage.columns.model"),
      cell: (r) => (
        <span className="font-mono text-xs">{r.model ?? "—"}</span>
      ),
    },
    {
      key: "provider",
      header: t("usage.columns.provider"),
      cell: (r) =>
        r.provider_id != null
          ? (providerMap.get(r.provider_id) ?? `#${r.provider_id}`)
          : "—",
    },
    {
      key: "tokens",
      header: `${t("usage.columns.inputTokens")} / ${t("usage.columns.outputTokens")}`,
      cell: (r) => (
        <span className="tabular-nums text-xs">
          {r.input_tokens} / {r.output_tokens}
        </span>
      ),
    },
    {
      key: "cost",
      header: t("usage.columns.cost"),
      cell: (r) => (
        <span className="tabular-nums text-xs">
          ${parseFloat(r.cost || "0").toFixed(5)}
        </span>
      ),
    },
    {
      key: "latency",
      header: t("usage.columns.latency"),
      cell: (r) => (
        <span className="tabular-nums text-xs">{r.latency_ms}ms</span>
      ),
    },
    {
      key: "badges",
      header: "",
      cell: (r) => (
        <div className="flex gap-1">
          <Badge variant="outline" className="text-xs">{r.usage_source}</Badge>
          <Badge variant="secondary" className="text-xs">{r.ended}</Badge>
        </div>
      ),
    },
  ];

  // Audit query + columns
  const [auditLimit, setAuditLimit] = useState(100);
  const { data: auditRows, isPending: auditPending } = useQuery(auditQuery(auditLimit));

  const auditCols: DataColumn<AuditLog>[] = [
    {
      key: "at",
      header: t("audit.columns.at"),
      cell: (r) => (
        <span className="whitespace-nowrap font-mono text-xs text-muted-foreground">
          {fmtAt(r.at)}
        </span>
      ),
    },
    { key: "actor", header: t("audit.columns.actor"), cell: (r) => r.actor_name ?? "—" },
    { key: "action", header: t("audit.columns.action"), cell: (r) => <span className="font-mono text-xs">{r.action}</span> },
    { key: "target", header: t("audit.columns.target"), cell: (r) => <span className="font-mono text-xs">{r.target}</span> },
    {
      key: "status",
      header: t("audit.columns.status"),
      cell: (r) => <Badge variant={r.status >= 400 ? "destructive" : "secondary"}>{r.status}</Badge>,
    },
    { key: "ip", header: t("audit.columns.sourceIp"), cell: (r) => r.source_ip ?? "—" },
  ];

  return (
    <div className="grid gap-4 p-4 md:p-6">
      <h1 className="text-xl font-semibold">{t("usage.explorer")}</h1>

      <Tabs defaultValue="usage">
        <TabsList>
          <TabsTrigger value="usage">{t("usage.tab.usage")}</TabsTrigger>
          <TabsTrigger value="audit">{t("usage.tab.audit")}</TabsTrigger>
        </TabsList>

        <TabsContent value="usage" className="mt-4 space-y-4">
          <UsageFilters value={filter} onChange={handleFilterChange} />

          {isPending ? (
            <div className="space-y-2" aria-busy="true">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-10" />
              ))}
            </div>
          ) : (
            <DataTable
              columns={usageCols}
              rows={rows}
              rowKey={(r) => r.id}
              empty={t("usage.empty")}
              onRowClick={openDrawer}
              renderCard={(r) => (
                <div className="grid gap-1">
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-mono text-xs">{r.operation}</span>
                    <span className="tabular-nums text-xs">${parseFloat(r.cost || "0").toFixed(5)}</span>
                  </div>
                  <div className="flex flex-wrap gap-1 text-xs text-muted-foreground">
                    <span>{fmtAt(r.at)}</span>
                    {r.model && <span>· {r.model}</span>}
                    {r.provider_id != null && <span>· {providerMap.get(r.provider_id) ?? `#${r.provider_id}`}</span>}
                    <span>· {r.latency_ms}ms</span>
                  </div>
                  <div className="flex gap-1">
                    <Badge variant="outline" className="text-xs">{r.usage_source}</Badge>
                    <Badge variant="secondary" className="text-xs">{r.ended}</Badge>
                  </div>
                </div>
              )}
            />
          )}

          {hasMore && rows.length > 0 && (
            <div className="flex justify-center pt-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => void loadMore()}
                disabled={loadingMore}
              >
                {loadingMore ? "…" : t("usage.loadMore")}
              </Button>
            </div>
          )}
        </TabsContent>

        <TabsContent value="audit" className="mt-4 space-y-4">
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">{t("audit.limit.label")}</span>
            {(["100", "500", "1000"] as const).map((n) => (
              <button
                key={n}
                type="button"
                onClick={() => setAuditLimit(Number(n))}
                className={
                  auditLimit === Number(n)
                    ? "rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground"
                    : "rounded-md border px-3 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                }
              >
                {t(`audit.limit.${n}`)}
              </button>
            ))}
          </div>

          {auditPending ? (
            <div className="space-y-2" aria-busy="true">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-10" />
              ))}
            </div>
          ) : (
            <DataTable
              columns={auditCols}
              rows={auditRows ?? []}
              rowKey={(r) => r.id}
              empty={t("audit.empty")}
              renderCard={(r) => (
                <div className="grid gap-1">
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-mono text-xs">{r.action}</span>
                    <Badge variant={r.status >= 400 ? "destructive" : "secondary"}>{r.status}</Badge>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    <span>{fmtAt(r.at)}</span>
                    {r.actor_name && <span> · {r.actor_name}</span>}
                    {r.source_ip && <span> · {r.source_ip}</span>}
                  </div>
                  <span className="font-mono text-xs text-muted-foreground">{r.target}</span>
                </div>
              )}
            />
          )}
        </TabsContent>
      </Tabs>

      <RequestDrawer
        open={drawerOpen}
        onOpenChange={setDrawerOpen}
        requestId={selectedRid}
      />
    </div>
  );
}

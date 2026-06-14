import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ruleSetsQuery, type RuleSet } from "@/api/rules";
import { DataTable, type DataColumn } from "@/components/data-table";
import { EntityDialog } from "@/components/entity-dialog";
import { RuleSetForm } from "@/components/rules/rule-set-form";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";

export const Route = createFileRoute("/_app/rules/")({
  loader: ({ context }) => context.queryClient.ensureQueryData(ruleSetsQuery),
  component: RuleSetsPage,
});

function RuleSetsPage() {
  const { t } = useTranslation("rules");
  const navigate = useNavigate();
  const { data: ruleSets, isPending } = useQuery(ruleSetsQuery);
  const [createOpen, setCreateOpen] = useState(false);

  const columns: DataColumn<RuleSet>[] = [
    { key: "name", header: t("ruleSet.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
    { key: "enabled", header: t("ruleSet.enabled"), cell: (r) => (
      <Badge variant={r.enabled ? "secondary" : "outline"}>{r.enabled ? "on" : "off"}</Badge>
    )},
    { key: "description", header: t("ruleSet.description"), cell: (r) => (
      <span className="max-w-xs truncate text-sm text-muted-foreground">{r.description ?? "—"}</span>
    )},
  ];

  return (
    <div className="grid gap-4 p-4 md:p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">{t("title")}</h1>
          <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
        </div>
        <Button onClick={() => setCreateOpen(true)}>
          <Plus className="size-4" aria-hidden />
          <span className="hidden sm:inline">{t("ruleSet.add")}</span>
        </Button>
      </div>
      {isPending ? (
        <div className="grid gap-2" aria-busy="true">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
      ) : (
        <DataTable
          columns={columns}
          rows={ruleSets ?? []}
          rowKey={(r) => r.id}
          empty={t("ruleSet.empty")}
          onRowClick={(r) => void navigate({ to: "/rules/$ruleSetId", params: { ruleSetId: String(r.id) } })}
          renderCard={(r) => (
            <div className="grid gap-1">
              <div className="flex items-center justify-between">
                <span className="font-medium">{r.name}</span>
                <Badge variant={r.enabled ? "secondary" : "outline"}>{r.enabled ? "on" : "off"}</Badge>
              </div>
              {r.description && (
                <p className="truncate text-xs text-muted-foreground">{r.description}</p>
              )}
            </div>
          )}
        />
      )}
      <EntityDialog open={createOpen} onOpenChange={setCreateOpen} title={t("ruleSet.add")}>
        <RuleSetForm onSaved={(saved) => {
          setCreateOpen(false);
          void navigate({ to: "/rules/$ruleSetId", params: { ruleSetId: String(saved.id) } });
        }} />
      </EntityDialog>
    </div>
  );
}

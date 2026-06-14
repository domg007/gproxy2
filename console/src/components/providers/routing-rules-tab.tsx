import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { routingRulesQuery, deleteRoutingRule, OPERATIONS, KINDS, type RoutingRule } from "@/api/rules";
import { ApiError } from "@/api/http";
import { ConfirmDangerous } from "@/components/confirm-dangerous";
import { DataTable, type DataColumn } from "@/components/data-table";
import { EntityDialog } from "@/components/entity-dialog";
import { RoutingRuleForm } from "@/components/rules/routing-rule-form";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";

type Op = typeof OPERATIONS[number];
type Kind = typeof KINDS[number];

function opLabel(op: string, t: (k: string) => string): string {
  return (OPERATIONS as readonly string[]).includes(op) ? t(`operation.${op as Op}`) : op;
}

function kindLabel(k: string, t: (key: string) => string): string {
  return (KINDS as readonly string[]).includes(k) ? t(`kind.${k as Kind}`) : k;
}

function destLabel(op: string | null, k: string | null, t: (key: string) => string): string {
  if (!op && !k) return "—";
  const opPart = op ? opLabel(op, t) : "—";
  const kPart = k ? kindLabel(k, t) : "—";
  return `${opPart} / ${kPart}`;
}

export function RoutingRulesTab({ providerId }: { providerId: number }) {
  const { t } = useTranslation("rules");
  const queryClient = useQueryClient();
  const { data: rules, isPending } = useQuery(routingRulesQuery(providerId));

  const [formOpen, setFormOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<RoutingRule | undefined>(undefined);
  const [deleteTarget, setDeleteTarget] = useState<RoutingRule | undefined>(undefined);

  const removal = useMutation({
    mutationFn: (id: number) => deleteRoutingRule(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["providers", providerId, "routing-rules"] });
      setDeleteTarget(undefined);
    },
    onError: (error) => {
      toast.error(error instanceof ApiError ? error.message : String(error));
      setDeleteTarget(undefined);
    },
  });

  const openCreate = () => { setEditTarget(undefined); setFormOpen(true); };
  const openEdit = (r: RoutingRule) => { setEditTarget(r); setFormOpen(true); };

  const actions = (r: RoutingRule) => (
    <div className="flex items-center justify-end gap-1">
      <Button variant="ghost" size="icon" aria-label={t("routingRule.operation")} onClick={(e) => { e.stopPropagation(); openEdit(r); }}>
        <Pencil className="size-4" aria-hidden />
      </Button>
      <Button variant="ghost" size="icon" className="text-destructive" aria-label={t("routingRule.deleteConfirm")}
        onClick={(e) => { e.stopPropagation(); setDeleteTarget(r); }}>
        <Trash2 className="size-4" aria-hidden />
      </Button>
    </div>
  );

  const columns: DataColumn<RoutingRule>[] = [
    { key: "sort_order", header: t("routingRule.sortOrder"), cell: (r) => r.sort_order },
    { key: "operation", header: t("routingRule.operation"), cell: (r) => opLabel(r.operation, t) },
    { key: "kind", header: t("routingRule.kind"), cell: (r) => kindLabel(r.kind, t) },
    { key: "implementation", header: t("routingRule.implementation"), cell: (r) => (
      <Badge variant="outline" className="font-mono text-xs">{t(`implementation.${r.implementation}`)}</Badge>
    ) },
    { key: "dest", header: `${t("routingRule.destOperation")} / ${t("routingRule.destKind")}`, cell: (r) => (
      <span className="text-xs text-muted-foreground">{destLabel(r.dest_operation, r.dest_kind, t)}</span>
    ) },
    { key: "enabled", header: t("routingRule.enabled"), cell: (r) => (
      <Badge variant={r.enabled ? "secondary" : "outline"}>{r.enabled ? "on" : "off"}</Badge>
    ) },
    { key: "actions", header: "", cell: actions, className: "w-20 text-right" },
  ];

  return (
    <div className="grid gap-3">
      <div className="flex items-center justify-end">
        <Button onClick={openCreate}>
          <Plus className="size-4" aria-hidden />
          {t("routingRule.add")}
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
          rows={rules ?? []}
          rowKey={(r) => r.id}
          empty={t("routingRule.empty")}
          onRowClick={openEdit}
          renderCard={(r) => (
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <span className="font-medium text-sm">{opLabel(r.operation, t)} / {kindLabel(r.kind, t)}</span>
                <Badge variant={r.enabled ? "secondary" : "outline"}>{r.enabled ? "on" : "off"}</Badge>
              </div>
              <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <Badge variant="outline" className="font-mono">{t(`implementation.${r.implementation}`)}</Badge>
                {(r.dest_operation ?? r.dest_kind) && (
                  <span>{destLabel(r.dest_operation, r.dest_kind, t)}</span>
                )}
                <span>#{r.sort_order}</span>
              </div>
              {actions(r)}
            </div>
          )}
        />
      )}

      <EntityDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        title={editTarget ? t("routingRule.operation") : t("routingRule.add")}
        wide
      >
        <RoutingRuleForm
          key={editTarget?.id ?? "new"}
          providerId={providerId}
          rule={editTarget}
          onSaved={() => setFormOpen(false)}
        />
      </EntityDialog>

      <ConfirmDangerous
        open={deleteTarget !== undefined}
        onOpenChange={(o) => { if (!o) setDeleteTarget(undefined); }}
        title={t("routingRule.deleteConfirm")}
        description={t("routingRule.deleteConfirm")}
        confirmLabel={t("routingRule.deleteConfirm")}
        onConfirm={() => { if (deleteTarget) removal.mutate(deleteTarget.id); }}
        pending={removal.isPending}
      />
    </div>
  );
}

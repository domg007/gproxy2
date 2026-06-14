import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providerRuleSetsQuery, ruleSetsQuery, deleteProviderRuleSet, type ProviderRuleSet } from "@/api/rules";
import { ApiError } from "@/api/http";
import { ConfirmDangerous } from "@/components/confirm-dangerous";
import { DataTable, type DataColumn } from "@/components/data-table";
import { EntityDialog } from "@/components/entity-dialog";
import { ProviderRuleSetForm } from "@/components/rules/provider-rule-set-form";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";

export function ProviderRuleSetsTab({ providerId }: { providerId: number }) {
  const { t } = useTranslation("rules");
  const queryClient = useQueryClient();
  const { data: attachments, isPending: attachPending } = useQuery(providerRuleSetsQuery(providerId));
  const { data: allRuleSets, isPending: rsPending } = useQuery(ruleSetsQuery);

  const [formOpen, setFormOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<ProviderRuleSet | undefined>(undefined);
  const [deleteTarget, setDeleteTarget] = useState<ProviderRuleSet | undefined>(undefined);

  const isPending = attachPending || rsPending;

  // Build id→name resolution map
  const rsMap = new Map((allRuleSets ?? []).map((rs) => [rs.id, rs.name]));
  const attachedIds = (attachments ?? []).map((a) => a.rule_set_id);

  const removal = useMutation({
    mutationFn: (id: number) => deleteProviderRuleSet(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["providers", providerId, "rule-sets"] });
      setDeleteTarget(undefined);
    },
    onError: (error) => {
      toast.error(error instanceof ApiError ? error.message : String(error));
      setDeleteTarget(undefined);
    },
  });

  const openCreate = () => { setEditTarget(undefined); setFormOpen(true); };
  const openEdit = (a: ProviderRuleSet) => { setEditTarget(a); setFormOpen(true); };

  const actions = (a: ProviderRuleSet) => (
    <div className="flex items-center justify-end gap-1">
      <Button variant="ghost" size="icon" aria-label={t("providerRuleSet.ruleSet")} onClick={(e) => { e.stopPropagation(); openEdit(a); }}>
        <Pencil className="size-4" aria-hidden />
      </Button>
      <Button variant="ghost" size="icon" className="text-destructive" aria-label={t("providerRuleSet.unattach")}
        onClick={(e) => { e.stopPropagation(); setDeleteTarget(a); }}>
        <Trash2 className="size-4" aria-hidden />
      </Button>
    </div>
  );

  const columns: DataColumn<ProviderRuleSet>[] = [
    { key: "name", header: t("providerRuleSet.ruleSet"), cell: (a) => (
      <span className="font-medium">{rsMap.get(a.rule_set_id) ?? `#${a.rule_set_id}`}</span>
    ) },
    { key: "sort_order", header: t("providerRuleSet.sortOrder"), cell: (a) => a.sort_order },
    { key: "enabled", header: t("providerRuleSet.enabled"), cell: (a) => (
      <Badge variant={a.enabled ? "secondary" : "outline"}>{a.enabled ? "on" : "off"}</Badge>
    ) },
    { key: "actions", header: "", cell: actions, className: "w-20 text-right" },
  ];

  return (
    <div className="grid gap-3">
      <div className="flex items-center justify-end">
        <Button onClick={openCreate}>
          <Plus className="size-4" aria-hidden />
          {t("providerRuleSet.attach")}
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
          rows={attachments ?? []}
          rowKey={(a) => a.id}
          empty={t("providerRuleSet.empty")}
          onRowClick={openEdit}
          renderCard={(a) => (
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <span className="font-medium">{rsMap.get(a.rule_set_id) ?? `#${a.rule_set_id}`}</span>
                <Badge variant={a.enabled ? "secondary" : "outline"}>{a.enabled ? "on" : "off"}</Badge>
              </div>
              <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <span>#{a.sort_order}</span>
              </div>
              {actions(a)}
            </div>
          )}
        />
      )}

      <EntityDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        title={editTarget ? t("providerRuleSet.ruleSet") : t("providerRuleSet.attach")}
      >
        <ProviderRuleSetForm
          key={editTarget?.id ?? "new"}
          providerId={providerId}
          attachment={editTarget}
          ruleSets={allRuleSets ?? []}
          attachedIds={attachedIds}
          onSaved={() => setFormOpen(false)}
        />
      </EntityDialog>

      <ConfirmDangerous
        open={deleteTarget !== undefined}
        onOpenChange={(o) => { if (!o) setDeleteTarget(undefined); }}
        title={t("providerRuleSet.unattach")}
        description={t("providerRuleSet.unattachConfirm")}
        confirmLabel={t("providerRuleSet.unattach")}
        onConfirm={() => { if (deleteTarget) removal.mutate(deleteTarget.id); }}
        pending={removal.isPending}
      />
    </div>
  );
}

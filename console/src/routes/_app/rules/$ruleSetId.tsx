import { useState } from "react";
import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, redirect, useNavigate } from "@tanstack/react-router";
import { Trash2, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  ruleSetQuery, rulesQuery, deleteRuleSet, deleteRule, type Rule,
} from "@/api/rules";
import { ApiError } from "@/api/http";
import { ConfirmDangerous } from "@/components/confirm-dangerous";
import { DataTable, type DataColumn } from "@/components/data-table";
import { EntityDialog } from "@/components/entity-dialog";
import { RuleSetForm } from "@/components/rules/rule-set-form";
import { RuleForm } from "@/components/rules/rule-form";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export const Route = createFileRoute("/_app/rules/$ruleSetId")({
  loader: ({ context, params }) => {
    const id = Number(params.ruleSetId);
    if (Number.isNaN(id)) throw redirect({ to: "/rules" });
    return context.queryClient.ensureQueryData(ruleSetQuery(id));
  },
  component: RuleSetDetailPage,
});

function RuleSetDetailPage() {
  const { ruleSetId } = Route.useParams();
  const id = Number(ruleSetId);
  const { t } = useTranslation("rules");
  const { t: tCommon } = useTranslation("common");
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { data: ruleSet } = useSuspenseQuery(ruleSetQuery(id));
  const { data: rules = [] } = useQuery(rulesQuery(id));

  const [editRsOpen, setEditRsOpen] = useState(false);
  const [deleteRsOpen, setDeleteRsOpen] = useState(false);
  const [addRuleOpen, setAddRuleOpen] = useState(false);
  const [editRule, setEditRule] = useState<Rule | null>(null);
  const [deleteRuleTarget, setDeleteRuleTarget] = useState<Rule | null>(null);

  const rsRemoval = useMutation({
    mutationFn: () => deleteRuleSet(id),
    onSuccess: () => {
      setDeleteRsOpen(false);
      void qc.invalidateQueries({ queryKey: ["rule-sets"] });
      void navigate({ to: "/rules" });
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });

  const ruleRemoval = useMutation({
    mutationFn: (rId: number) => deleteRule(rId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["rule-sets", id, "rules"] });
      setDeleteRuleTarget(null);
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });

  const sorted = [...rules].sort((a, b) => a.sort_order - b.sort_order);

  const columns: DataColumn<Rule>[] = [
    { key: "sort_order", header: t("rule.sortOrder"), cell: (r) => r.sort_order, className: "w-16" },
    { key: "kind", header: t("rule.kind"), cell: (r) => t(`kind.${r.kind}`) },
    { key: "config", header: t("rule.configJson"), cell: (r) => {
      const s = JSON.stringify(r.config_json);
      return <span className="font-mono text-xs">{s.length > 60 ? s.slice(0, 60) + "…" : s}</span>;
    }},
    { key: "filter", header: t("rule.filterModelPattern"), cell: (r) => r.filter_model_pattern ?? "—" },
    { key: "enabled", header: t("rule.enabled"), cell: (r) => (
      <Badge variant={r.enabled ? "secondary" : "outline"}>{r.enabled ? "on" : "off"}</Badge>
    )},
    { key: "delete", header: "", className: "w-10", cell: (r) => (
      <Button
        variant="ghost"
        size="sm"
        className="text-destructive"
        onClick={(e) => { e.stopPropagation(); setDeleteRuleTarget(r); }}
      >
        <Trash2 className="size-4" />
      </Button>
    )},
  ];

  return (
    <div className="grid gap-4 p-4 md:p-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold">{ruleSet.name}</h1>
          <Badge variant={ruleSet.enabled ? "secondary" : "outline"}>{ruleSet.enabled ? "on" : "off"}</Badge>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => setEditRsOpen(true)}>
            {tCommon("actions.edit")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="text-destructive"
            onClick={() => setDeleteRsOpen(true)}
          >
            <Trash2 className="size-4" />
          </Button>
        </div>
      </div>

      {ruleSet.description && (
        <p className="text-sm text-muted-foreground">{ruleSet.description}</p>
      )}

      <div className="flex items-center justify-between">
        <h2 className="font-medium">{t("rule.list")}</h2>
        <Button size="sm" onClick={() => setAddRuleOpen(true)}>
          <Plus className="size-4" aria-hidden />
          <span className="hidden sm:inline">{t("rule.add")}</span>
        </Button>
      </div>

      <DataTable
        columns={columns}
        rows={sorted}
        rowKey={(r) => r.id}
        empty={t("rule.empty")}
        onRowClick={(r) => setEditRule(r)}
        renderCard={(r) => (
          <div className="grid gap-1">
            <div className="flex items-center justify-between">
              <span className="font-medium">{t(`kind.${r.kind}`)}</span>
              <div className="flex items-center gap-2">
                <Badge variant={r.enabled ? "secondary" : "outline"}>{r.enabled ? "on" : "off"}</Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-destructive"
                  onClick={(e) => { e.stopPropagation(); setDeleteRuleTarget(r); }}
                >
                  <Trash2 className="size-3" />
                </Button>
              </div>
            </div>
            <p className="font-mono text-xs text-muted-foreground">
              {(() => { const s = JSON.stringify(r.config_json); return s.length > 60 ? s.slice(0, 60) + "…" : s; })()}
            </p>
            <p className="text-xs text-muted-foreground">#{r.sort_order}</p>
          </div>
        )}
      />

      <EntityDialog open={editRsOpen} onOpenChange={setEditRsOpen} title={ruleSet.name}>
        <RuleSetForm ruleSet={ruleSet} onSaved={() => {
          setEditRsOpen(false);
          void qc.invalidateQueries({ queryKey: ["rule-sets", id] });
        }} />
      </EntityDialog>

      <EntityDialog open={addRuleOpen} onOpenChange={setAddRuleOpen} title={t("rule.add")} wide>
        <RuleForm ruleSetId={id} onSaved={() => {
          setAddRuleOpen(false);
          void qc.invalidateQueries({ queryKey: ["rule-sets", id, "rules"] });
        }} />
      </EntityDialog>

      <EntityDialog open={editRule !== null} onOpenChange={(o) => { if (!o) setEditRule(null); }} title={editRule ? t(`kind.${editRule.kind}`) : t("rule.add")} wide>
        {editRule && (
          <RuleForm ruleSetId={id} rule={editRule} onSaved={() => {
            setEditRule(null);
            void qc.invalidateQueries({ queryKey: ["rule-sets", id, "rules"] });
          }} />
        )}
      </EntityDialog>

      <ConfirmDangerous
        open={deleteRsOpen}
        onOpenChange={setDeleteRsOpen}
        title={ruleSet.name}
        description={t("ruleSet.deleteConfirm")}
        confirmLabel={tCommon("actions.delete")}
        onConfirm={() => rsRemoval.mutate()}
        pending={rsRemoval.isPending}
      />

      <ConfirmDangerous
        open={deleteRuleTarget !== null}
        onOpenChange={(o) => { if (!o) setDeleteRuleTarget(null); }}
        title={deleteRuleTarget ? t(`kind.${deleteRuleTarget.kind}`) : ""}
        description={t("rule.deleteConfirm")}
        confirmLabel={tCommon("actions.delete")}
        onConfirm={() => deleteRuleTarget && ruleRemoval.mutate(deleteRuleTarget.id)}
        pending={ruleRemoval.isPending}
      />
    </div>
  );
}

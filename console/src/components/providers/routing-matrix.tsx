import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  effectiveRoutingQuery, routingRulesQuery, deleteRoutingRule, type RoutingRule,
} from "@/api/rules";
import { ApiError } from "@/api/http";
import { EntityDialog } from "@/components/entity-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { RoutingCellEditor, type CellInitial } from "./routing-cell-editor";

interface Row {
  operation: string;
  kind: string;
  implementation: string;
  destKind: string | null;
  rule?: RoutingRule;
  isCell: boolean; // one of the computed default cells (vs an extra rule)
}

function behaviorBadge(impl: string, destKind: string | null, t: (k: string) => string) {
  const label = t(`implementation.${impl}`);
  if (impl === "transform_to") {
    return (
      <Badge variant="outline" className="font-mono text-xs text-blue-700 border-blue-300 dark:text-blue-400 dark:border-blue-700">
        {label}{destKind ? ` → ${t(`protocolKind.${destKind}`)}` : ""}
      </Badge>
    );
  }
  if (impl === "passthrough") {
    return <Badge variant="secondary" className="font-mono text-xs bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400">{label}</Badge>;
  }
  if (impl === "local") {
    return <Badge variant="outline" className="font-mono text-xs text-amber-700 border-amber-300 dark:text-amber-400 dark:border-amber-700">{label}</Badge>;
  }
  return <Badge variant="destructive" className="font-mono text-xs">{label}</Badge>;
}

export function RoutingMatrix({ providerId }: { providerId: number }) {
  const { t } = useTranslation("rules");
  const queryClient = useQueryClient();
  const { data: effective, isPending: effPending } = useQuery(effectiveRoutingQuery(providerId));
  const { data: rules, isPending: rulesPending } = useQuery(routingRulesQuery(providerId));

  const [editorOpen, setEditorOpen] = useState(false);
  const [target, setTarget] = useState<{ mode: "add" | "edit"; initial: CellInitial } | null>(null);

  const removal = useMutation({
    mutationFn: (id: number) => deleteRoutingRule(id),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["providers", providerId, "routing-rules"] }),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });

  const rows = useMemo<Row[]>(() => {
    const sorted = [...(rules ?? [])].sort((a, b) => a.sort_order - b.sort_order);
    const ruleByCell = new Map<string, RoutingRule>();
    for (const r of sorted) {
      const key = `${r.operation}:${r.kind}`;
      if (!ruleByCell.has(key)) ruleByCell.set(key, r);
    }
    const cellKeys = new Set((effective ?? []).map((c) => `${c.operation}:${c.kind}`));
    const base: Row[] = (effective ?? []).map((c) => ({
      operation: c.operation,
      kind: c.kind,
      implementation: c.implementation,
      destKind: c.dest_kind,
      rule: ruleByCell.get(`${c.operation}:${c.kind}`),
      isCell: true,
    }));
    const extras: Row[] = [];
    const seen = new Set<string>();
    for (const r of sorted) {
      const key = `${r.operation}:${r.kind}`;
      if (cellKeys.has(key) || seen.has(key)) continue;
      seen.add(key);
      extras.push({ operation: r.operation, kind: r.kind, implementation: r.implementation, destKind: r.dest_kind, rule: r, isCell: false });
    }
    return [...base, ...extras];
  }, [effective, rules]);

  const openEdit = (row: Row) => {
    setTarget({
      mode: "edit",
      initial: {
        operation: row.operation,
        kind: row.kind,
        implementation: row.rule?.implementation ?? row.implementation,
        destKind: row.rule?.dest_kind ?? row.destKind,
        ruleId: row.rule?.id,
        sortOrder: row.rule?.sort_order,
      },
    });
    setEditorOpen(true);
  };
  const openAdd = () => {
    setTarget({ mode: "add", initial: { operation: "generate_content", kind: "open_ai_chat_completions", implementation: "passthrough", destKind: "claude_messages" } });
    setEditorOpen(true);
  };

  const isPending = effPending || rulesPending;

  return (
    <section className="grid gap-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold">{t("routing.title")}</h3>
          <p className="text-xs text-muted-foreground mt-1">{t("routing.caption")}</p>
        </div>
        <Button onClick={openAdd} className="shrink-0">
          <Plus className="size-4" aria-hidden />
          {t("routing.add")}
        </Button>
      </div>

      {isPending ? (
        <div className="grid gap-2" aria-busy="true">
          {Array.from({ length: 4 }).map((_, i) => <Skeleton key={i} className="h-9" />)}
        </div>
      ) : (
        <div className="overflow-x-auto rounded-md border">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b bg-muted/50">
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("routing.columns.operation")}</th>
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("routing.columns.kind")}</th>
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("routing.columns.behavior")}</th>
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("routing.columns.source")}</th>
                <th className="w-20 px-3 py-2" />
              </tr>
            </thead>
            <tbody>
              {rows.map((row, i) => (
                <tr
                  key={`${row.operation}:${row.kind}`}
                  onClick={() => openEdit(row)}
                  className={cn(
                    "cursor-pointer hover:bg-accent/50",
                    i % 2 === 0 ? "bg-background" : "bg-muted/20",
                    row.rule && "bg-primary/5 font-medium",
                  )}
                >
                  <td className="px-3 py-2">{t(`operation.${row.operation}`)}</td>
                  <td className="px-3 py-2">{t(`protocolKind.${row.kind}`)}</td>
                  <td className="px-3 py-2">{behaviorBadge(row.implementation, row.destKind, t)}</td>
                  <td className="px-3 py-2">
                    {row.rule
                      ? <Badge variant="default" className="text-xs">{t("routing.source.custom")}</Badge>
                      : <Badge variant="outline" className="text-xs text-muted-foreground">{t("routing.source.default")}</Badge>}
                  </td>
                  <td className="px-3 py-2">
                    <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button variant="ghost" size="icon" aria-label={t("routing.editTitle")} onClick={() => openEdit(row)}>
                        <Pencil className="size-4" aria-hidden />
                      </Button>
                      {row.rule && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="text-muted-foreground"
                          aria-label={row.isCell ? t("routing.reset") : t("routing.delete")}
                          title={row.isCell ? t("routing.reset") : t("routing.delete")}
                          disabled={removal.isPending}
                          onClick={() => removal.mutate(row.rule!.id)}
                        >
                          {row.isCell ? <RotateCcw className="size-4" aria-hidden /> : <Trash2 className="size-4" aria-hidden />}
                        </Button>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <EntityDialog
        open={editorOpen}
        onOpenChange={(o) => { setEditorOpen(o); if (!o) setTarget(null); }}
        title={target?.mode === "add" ? t("routing.addTitle") : t("routing.editTitle")}
      >
        {target && (
          <RoutingCellEditor
            key={`${target.mode}:${target.initial.operation}:${target.initial.kind}`}
            providerId={providerId}
            mode={target.mode}
            initial={target.initial}
            onSaved={() => { setEditorOpen(false); setTarget(null); }}
          />
        )}
      </EntityDialog>
    </section>
  );
}

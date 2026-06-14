import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { effectiveRoutingQuery, type EffectiveRoute } from "@/api/rules";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

function implBadge(row: EffectiveRoute, t: (k: string) => string) {
  const label = t(`implementation.${row.implementation}`);
  if (row.implementation === "passthrough") {
    return <Badge variant="secondary" className="font-mono text-xs bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400">{label}</Badge>;
  }
  if (row.implementation === "transform_to") {
    const dest = row.dest_kind ? t(`protocolKind.${row.dest_kind}`) : row.dest_kind;
    return (
      <Badge variant="outline" className="font-mono text-xs text-blue-700 border-blue-300 dark:text-blue-400 dark:border-blue-700">
        {label}{dest ? ` → ${dest}` : ""}
      </Badge>
    );
  }
  if (row.implementation === "local") {
    return <Badge variant="outline" className="font-mono text-xs text-amber-700 border-amber-300 dark:text-amber-400 dark:border-amber-700">{label}</Badge>;
  }
  // unsupported
  return <Badge variant="destructive" className="font-mono text-xs">{label}</Badge>;
}

function sourceBadge(source: EffectiveRoute["source"], t: (k: string) => string) {
  if (source === "override") {
    return <Badge variant="default" className="text-xs">{t("effective.source.override")}</Badge>;
  }
  return <Badge variant="outline" className="text-xs text-muted-foreground">{t("effective.source.default")}</Badge>;
}

export function EffectiveRoutingTable({ providerId }: { providerId: number }) {
  const { t } = useTranslation("rules");
  const { data, isPending } = useQuery(effectiveRoutingQuery(providerId));

  return (
    <section className="grid gap-3">
      <div>
        <h3 className="text-sm font-semibold">{t("effective.title")}</h3>
        <p className="text-xs text-muted-foreground mt-1">{t("effective.caption")}</p>
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
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("effective.columns.operation")}</th>
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("effective.columns.kind")}</th>
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("effective.columns.impl")}</th>
                <th className="px-3 py-2 text-left font-medium text-muted-foreground">{t("effective.columns.source")}</th>
              </tr>
            </thead>
            <tbody>
              {(data ?? []).map((row, i) => (
                <tr
                  key={`${row.operation}:${row.kind}`}
                  className={cn(
                    i % 2 === 0 ? "bg-background" : "bg-muted/20",
                    row.source === "override" && "bg-primary/5 font-medium"
                  )}
                >
                  <td className="px-3 py-2">{t(`operation.${row.operation}`)}</td>
                  <td className="px-3 py-2">{t(`protocolKind.${row.kind}`)}</td>
                  <td className="px-3 py-2">{implBadge(row, t)}</td>
                  <td className="px-3 py-2">{sourceBadge(row.source, t)}</td>
                </tr>
              ))}
              {!data?.length && (
                <tr>
                  <td colSpan={4} className="px-3 py-6 text-center text-xs text-muted-foreground">—</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

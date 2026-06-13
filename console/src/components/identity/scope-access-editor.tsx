import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  type Scope, permissionsQuery, rateLimitsQuery, quotaQuery,
  upsertPermission, deletePermission, upsertRateLimit, deleteRateLimit, upsertQuota, deleteQuota,
  type RoutePermission, type RateLimit,
} from "@/api/authz";
import { ApiError } from "@/api/http";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";

export function ScopeAccessEditor({ scope, scopeId }: { scope: Scope; scopeId: number }) {
  return (
    <div className="grid gap-6">
      <PermissionsSection scope={scope} scopeId={scopeId} />
      <Separator />
      <RateLimitsSection scope={scope} scopeId={scopeId} />
      <Separator />
      <QuotaSection scope={scope} scopeId={scopeId} />
    </div>
  );
}

function PermissionsSection({ scope, scopeId }: { scope: Scope; scopeId: number }) {
  const { t } = useTranslation("identity");
  const qc = useQueryClient();
  const key = ["route-permissions", scope, scopeId];
  const { data } = useQuery(permissionsQuery(scope, scopeId));
  const [pattern, setPattern] = useState("");
  const add = useMutation({
    mutationFn: () => upsertPermission({ scope, scope_id: scopeId, route_pattern: pattern.trim() }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: key }); setPattern(""); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  const del = useMutation({
    mutationFn: (id: number) => deletePermission(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: key }),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  return (
    <section className="grid gap-2">
      <div>
        <h3 className="text-sm font-medium">{t("access.permissions")}</h3>
        <p className="text-xs text-muted-foreground">{t("access.permissionsHint")}</p>
      </div>
      {(data ?? []).length === 0 && <p className="text-sm text-muted-foreground">{t("access.noPermissions")}</p>}
      <ul className="grid gap-1">
        {(data ?? []).map((p: RoutePermission) => (
          <li key={p.id} className="flex items-center justify-between rounded-md border px-3 py-1.5">
            <span className="font-mono text-sm">{p.route_pattern}</span>
            <Button variant="ghost" size="icon" className="text-destructive" aria-label={t("access.deletePermission")} onClick={() => del.mutate(p.id)}>
              <Trash2 className="size-4" aria-hidden />
            </Button>
          </li>
        ))}
      </ul>
      <form className="flex items-end gap-2" onSubmit={(e) => { e.preventDefault(); if (pattern.trim()) add.mutate(); }}>
        <div className="grid flex-1 gap-1">
          <Label htmlFor={`perm-${scope}-${scopeId}`} className="sr-only">{t("access.routePattern")}</Label>
          <Input id={`perm-${scope}-${scopeId}`} value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder="gpt-4o / claude-* / *" />
        </div>
        <Button type="submit" disabled={add.isPending || !pattern.trim()}>
          <Plus className="size-4" aria-hidden />
          {t("access.addPermission")}
        </Button>
      </form>
    </section>
  );
}

function RateLimitsSection({ scope, scopeId }: { scope: Scope; scopeId: number }) {
  const { t } = useTranslation("identity");
  const qc = useQueryClient();
  const key = ["rate-limits", scope, scopeId];
  const { data } = useQuery(rateLimitsQuery(scope, scopeId));
  const [pattern, setPattern] = useState("");
  const [rpm, setRpm] = useState(""); const [rpd, setRpd] = useState(""); const [tot, setTot] = useState("");
  const intOrNull = (v: string) => { const n = Number(v); return v.trim() !== "" && Number.isInteger(n) && n >= 0 ? n : null; };
  const add = useMutation({
    mutationFn: () => upsertRateLimit({ scope, scope_id: scopeId, route_pattern: pattern.trim() || "*", rpm: intOrNull(rpm), rpd: intOrNull(rpd), total_tokens: intOrNull(tot) }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: key }); setPattern(""); setRpm(""); setRpd(""); setTot(""); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  const del = useMutation({ mutationFn: (id: number) => deleteRateLimit(id), onSuccess: () => void qc.invalidateQueries({ queryKey: key }), onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)) });
  return (
    <section className="grid gap-2">
      <div>
        <h3 className="text-sm font-medium">{t("access.rateLimits")}</h3>
        <p className="text-xs text-muted-foreground">{t("access.rateLimitsHint")}</p>
      </div>
      {(data ?? []).length === 0 && <p className="text-sm text-muted-foreground">{t("access.noRateLimits")}</p>}
      <ul className="grid gap-1">
        {(data ?? []).map((r: RateLimit) => (
          <li key={r.id} className="flex items-center justify-between rounded-md border px-3 py-1.5 text-sm">
            <span className="font-mono">{r.route_pattern}</span>
            <span className="flex items-center gap-3 text-muted-foreground">
              <span>{t("access.rpm")} {r.rpm ?? "—"}</span>
              <span>{t("access.rpd")} {r.rpd ?? "—"}</span>
              <span>{t("access.totalTokens")} {r.total_tokens ?? "—"}</span>
              <Button variant="ghost" size="icon" className="text-destructive" aria-label={t("access.deleteRateLimit")} onClick={() => del.mutate(r.id)}>
                <Trash2 className="size-4" aria-hidden />
              </Button>
            </span>
          </li>
        ))}
      </ul>
      <form className="grid gap-2 sm:grid-cols-[1fr_auto_auto_auto_auto] sm:items-end" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <div className="grid gap-1">
          <Label htmlFor={`rl-pat-${scope}-${scopeId}`} className="text-xs">{t("access.routePattern")}</Label>
          <Input id={`rl-pat-${scope}-${scopeId}`} value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder="*" />
        </div>
        <div className="grid gap-1"><Label className="text-xs">{t("access.rpm")}</Label><Input inputMode="numeric" value={rpm} onChange={(e) => setRpm(e.target.value)} className="w-20" /></div>
        <div className="grid gap-1"><Label className="text-xs">{t("access.rpd")}</Label><Input inputMode="numeric" value={rpd} onChange={(e) => setRpd(e.target.value)} className="w-20" /></div>
        <div className="grid gap-1"><Label className="text-xs">{t("access.totalTokens")}</Label><Input inputMode="numeric" value={tot} onChange={(e) => setTot(e.target.value)} className="w-24" /></div>
        <Button type="submit" disabled={add.isPending}>
          <Plus className="size-4" aria-hidden />
          {t("access.addRateLimit")}
        </Button>
      </form>
    </section>
  );
}

function QuotaSection({ scope, scopeId }: { scope: Scope; scopeId: number }) {
  const { t } = useTranslation("identity");
  const { t: tc } = useTranslation("common");
  const qc = useQueryClient();
  const key = ["quotas", scope, scopeId];
  const { data: quota } = useQuery(quotaQuery(scope, scopeId));
  const [total, setTotal] = useState("");
  const save = useMutation({
    mutationFn: async () => {
      // Re-fetch the freshest quota immediately before upsert: cost_used is a billing-owned
      // accumulator the server increments — sending a stale cached value would overwrite/erase
      // accumulated spend (the backend writes the input cost_used directly on update).
      const fresh = await qc.fetchQuery(quotaQuery(scope, scopeId));
      return upsertQuota({ id: fresh?.id ?? quota?.id ?? null, scope, scope_id: scopeId, quota_total: total.trim(), cost_used: fresh?.cost_used ?? "0" });
    },
    onSuccess: () => { void qc.invalidateQueries({ queryKey: key }); setTotal(""); toast.success(tc("actions.save")); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  const clear = useMutation({
    mutationFn: () => { if (!quota) return Promise.resolve(); return deleteQuota(quota.id); },
    onSuccess: () => void qc.invalidateQueries({ queryKey: key }),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  return (
    <section className="grid gap-2">
      <div>
        <h3 className="text-sm font-medium">{t("access.quota")}</h3>
        <p className="text-xs text-muted-foreground">{t("access.quotaHint")}</p>
      </div>
      {quota ? (
        <p className="text-sm text-muted-foreground">
          {t("access.costUsed")}: <span className="font-mono">{quota.cost_used}</span> / <span className="font-mono">{quota.quota_total}</span>
        </p>
      ) : (
        <p className="text-sm text-muted-foreground">{t("access.noQuota")}</p>
      )}
      <form className="flex items-end gap-2" onSubmit={(e) => { e.preventDefault(); if (total.trim()) save.mutate(); }}>
        <div className="grid flex-1 gap-1">
          <Label htmlFor={`q-${scope}-${scopeId}`} className="text-xs">{t("access.quotaTotal")}</Label>
          <Input id={`q-${scope}-${scopeId}`} inputMode="decimal" value={total} onChange={(e) => setTotal(e.target.value)} placeholder={quota?.quota_total ?? "100.00"} />
        </div>
        <Button type="submit" disabled={save.isPending || !total.trim()}>{t("access.setQuota")}</Button>
        {quota && (
          <Button type="button" variant="ghost" className="text-destructive" disabled={clear.isPending} onClick={() => clear.mutate()}>
            {t("access.clearQuota")}
          </Button>
        )}
      </form>
    </section>
  );
}

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { upsertRoutingRule, OPERATIONS, KINDS, IMPLEMENTATIONS, type RoutingRule } from "@/api/rules";
import { ApiError } from "@/api/http";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";

interface Props {
  providerId: number;
  rule?: RoutingRule;
  onSaved?: (r: RoutingRule) => void;
}

type Op = typeof OPERATIONS[number];
type Kind = typeof KINDS[number];

// Radix Select forbids an empty-string item value; use a sentinel for the
// "inherit" (null) choice on the optional dest_* selects and map it back to "".
const INHERIT = "__inherit__";

/** Returns OPERATIONS ∪ {current} so a legacy value isn't silently dropped. */
function operationOptions(current?: string | null): string[] {
  const set = [...OPERATIONS] as string[];
  if (current && !set.includes(current)) set.push(current);
  return set;
}

/** Returns KINDS ∪ {current} so a legacy value isn't silently dropped. */
function kindOptions(current?: string | null): string[] {
  const set = [...KINDS] as string[];
  if (current && !set.includes(current)) set.push(current);
  return set;
}

function isLegacy(value: string, set: readonly string[]): boolean {
  return !set.includes(value);
}

export function RoutingRuleForm({ providerId, rule, onSaved }: Props) {
  const { t } = useTranslation("rules");
  const qc = useQueryClient();

  const [operation, setOperation] = useState<string>(rule?.operation ?? OPERATIONS[0]);
  const [kind, setKind] = useState<string>(rule?.kind ?? KINDS[0]);
  const [implementation, setImplementation] = useState<string>(rule?.implementation ?? IMPLEMENTATIONS[0]);
  const [destOperation, setDestOperation] = useState<string>(rule?.dest_operation ?? "");
  const [destKind, setDestKind] = useState<string>(rule?.dest_kind ?? "");
  const [sortOrder, setSortOrder] = useState(String(rule?.sort_order ?? 0));
  const [enabled, setEnabled] = useState(rule?.enabled ?? true);
  const [formError, setFormError] = useState<string | null>(null);

  const showDest = implementation === "transform_to";

  const mutation = useMutation({
    mutationFn: () => {
      const orderNum = Number(sortOrder);
      if (!Number.isFinite(orderNum)) throw new ApiError(0, "bad_request", t("validation.sortOrderRequired"));

      return upsertRoutingRule(providerId, {
        id: rule?.id ?? null,
        provider_id: providerId,
        operation,
        kind,
        implementation,
        dest_operation: showDest && destOperation ? destOperation : null,
        dest_kind: showDest && destKind ? destKind : null,
        sort_order: orderNum,
        enabled,
      });
    },
    onSuccess: (result) => {
      void qc.invalidateQueries({ queryKey: ["providers", providerId, "routing-rules"] });
      toast.success(t("common.save"));
      setFormError(null);
      onSaved?.(result);
    },
    onError: (e) => {
      if (e instanceof ApiError && e.status === 409) {
        setFormError(e.message);
      } else {
        setFormError(e instanceof ApiError ? e.message : String(e));
      }
    },
  });

  const opOptions = operationOptions(rule?.operation);
  const kOptions = kindOptions(rule?.kind);
  const destOpOptions = operationOptions(rule?.dest_operation);
  const destKOptions = kindOptions(rule?.dest_kind);

  return (
    <form
      className="grid gap-4"
      onSubmit={(e) => { e.preventDefault(); setFormError(null); mutation.mutate(); }}
    >
      <div className="grid gap-1">
        <Label htmlFor="rr-operation">{t("routingRule.operation")}</Label>
        <Select value={operation} onValueChange={setOperation}>
          <SelectTrigger id="rr-operation"><SelectValue /></SelectTrigger>
          <SelectContent>
            {opOptions.map((op) => (
              <SelectItem key={op} value={op}>
                {isLegacy(op, OPERATIONS)
                  ? `(${t("routingRule.legacy")}) ${op}`
                  : t(`operation.${op as Op}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-1">
        <Label htmlFor="rr-kind">{t("routingRule.kind")}</Label>
        <Select value={kind} onValueChange={setKind}>
          <SelectTrigger id="rr-kind"><SelectValue /></SelectTrigger>
          <SelectContent>
            {kOptions.map((k) => (
              <SelectItem key={k} value={k}>
                {isLegacy(k, KINDS)
                  ? `(${t("routingRule.legacy")}) ${k}`
                  : t(`protocolKind.${k as Kind}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-1">
        <Label htmlFor="rr-impl">{t("routingRule.implementation")}</Label>
        <Select value={implementation} onValueChange={setImplementation}>
          <SelectTrigger id="rr-impl"><SelectValue /></SelectTrigger>
          <SelectContent>
            {IMPLEMENTATIONS.map((i) => (
              <SelectItem key={i} value={i}>{t(`implementation.${i}`)}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {showDest && (
        <>
          <div className="grid gap-1">
            <Label htmlFor="rr-dest-op">{t("routingRule.destOperation")}</Label>
            <Select value={destOperation || INHERIT} onValueChange={(v) => setDestOperation(v === INHERIT ? "" : v)}>
              <SelectTrigger id="rr-dest-op"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={INHERIT}>{t("routingRule.inherit")}</SelectItem>
                {destOpOptions.map((op) => (
                  <SelectItem key={op} value={op}>
                    {isLegacy(op, OPERATIONS)
                      ? `(${t("routingRule.legacy")}) ${op}`
                      : t(`operation.${op as Op}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid gap-1">
            <Label htmlFor="rr-dest-kind">{t("routingRule.destKind")}</Label>
            <Select value={destKind || INHERIT} onValueChange={(v) => setDestKind(v === INHERIT ? "" : v)}>
              <SelectTrigger id="rr-dest-kind"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={INHERIT}>{t("routingRule.inherit")}</SelectItem>
                {destKOptions.map((k) => (
                  <SelectItem key={k} value={k}>
                    {isLegacy(k, KINDS)
                      ? `(${t("routingRule.legacy")}) ${k}`
                      : t(`protocolKind.${k as Kind}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </>
      )}

      <div className="grid gap-1">
        <Label htmlFor="rr-sort">{t("routingRule.sortOrder")}</Label>
        <Input
          id="rr-sort"
          type="number"
          value={sortOrder}
          onChange={(e) => setSortOrder(e.target.value)}
        />
      </div>

      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="rr-enabled">{t("routingRule.enabled")}</Label>
        <Switch id="rr-enabled" checked={enabled} onCheckedChange={setEnabled} />
      </div>

      {formError && (
        <p role="alert" className="text-sm text-destructive">{formError}</p>
      )}
      <Button type="submit" disabled={mutation.isPending}>
        {mutation.isPending ? "…" : t("common.save")}
      </Button>
    </form>
  );
}

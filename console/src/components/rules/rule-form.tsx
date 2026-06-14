import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { upsertRule, RULE_KINDS, type Rule } from "@/api/rules";
import { ApiError } from "@/api/http";
import { JsonField, parseJsonText } from "@/components/form/json-field";
import { RuleConfigFields } from "./rule-config-fields";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";

interface Props {
  ruleSetId: number;
  rule?: Rule;
  onSaved?: (r: Rule) => void;
}

function validateConfig(kind: string, cfg: unknown): string | null {
  const c = cfg as Record<string, unknown> | null | undefined;
  if (!c) return `config required`;
  if (kind === "system_text" && !c.text) return `config.text is required`;
  if (kind === "rewrite" && (!c.path || !c.action)) return `config.path and action are required`;
  if (kind === "sanitize" && !c.pattern) return `config.pattern is required`;
  if (kind === "cache_breakpoint" && !c.target) return `config.target is required`;
  if (kind === "header" && !c.name) return `config.name is required`;
  return null;
}

export function RuleForm({ ruleSetId, rule, onSaved }: Props) {
  const { t } = useTranslation("rules");
  const qc = useQueryClient();

  const [kind, setKind] = useState(rule?.kind ?? "system_text");
  const [sortOrder, setSortOrder] = useState(String(rule?.sort_order ?? 0));
  const [enabled, setEnabled] = useState(rule?.enabled ?? true);
  const [filterModelPattern, setFilterModelPattern] = useState(rule?.filter_model_pattern ?? "");
  const [fopText, setFopText] = useState(
    rule?.filter_operation_keys != null
      ? JSON.stringify(rule.filter_operation_keys, null, 2)
      : "",
  );
  const [configValue, setConfigValue] = useState<unknown>(rule?.config_json ?? {});
  const [configValid, setConfigValid] = useState(true);
  const [formError, setFormError] = useState<string | null>(null);

  const handleKindChange = (k: string) => {
    setKind(k);
    setConfigValue({});
  };

  const mutation = useMutation({
    mutationFn: () => {
      const orderNum = Number(sortOrder);
      if (!Number.isInteger(orderNum)) throw new ApiError(0, "bad_request", "sort_order must be an integer");
      if (!configValid) throw new ApiError(0, "bad_request", "Invalid JSON in config");
      const cfgErr = validateConfig(kind, configValue);
      if (cfgErr) throw new ApiError(0, "bad_request", cfgErr);

      const fopParsed = fopText.trim()
        ? parseJsonText(fopText)
        : { ok: true as const, value: null };

      return upsertRule(ruleSetId, {
        id: rule?.id ?? null,
        rule_set_id: ruleSetId,
        kind,
        config_json: configValue,
        filter_model_pattern: filterModelPattern.trim() || null,
        filter_operation_keys: fopParsed.ok ? fopParsed.value : null,
        sort_order: orderNum,
        enabled,
      });
    },
    onSuccess: (result) => {
      void qc.invalidateQueries({ queryKey: ["rule-sets", ruleSetId, "rules"] });
      toast.success(t("common.save"));
      setFormError(null);
      onSaved?.(result);
    },
    onError: (e) => {
      setFormError(e instanceof ApiError ? e.message : String(e));
    },
  });

  return (
    <form
      className="grid gap-4"
      onSubmit={(e) => { e.preventDefault(); setFormError(null); mutation.mutate(); }}
    >
      <div className="grid gap-1">
        <Label htmlFor="rule-kind">{t("rule.kind")}</Label>
        <Select value={kind} onValueChange={handleKindChange}>
          <SelectTrigger id="rule-kind"><SelectValue /></SelectTrigger>
          <SelectContent>
            {RULE_KINDS.map((k) => (
              <SelectItem key={k} value={k}>{t(`kind.${k}`)}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="grid gap-1">
        <Label htmlFor="rule-sort">{t("rule.sortOrder")}</Label>
        <Input
          id="rule-sort"
          type="number"
          value={sortOrder}
          onChange={(e) => setSortOrder(e.target.value)}
        />
      </div>
      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="rule-enabled">{t("rule.enabled")}</Label>
        <Switch id="rule-enabled" checked={enabled} onCheckedChange={setEnabled} />
      </div>
      <div className="grid gap-1">
        <Label htmlFor="rule-fmp">{t("rule.filterModelPattern")}</Label>
        <Input
          id="rule-fmp"
          value={filterModelPattern}
          onChange={(e) => setFilterModelPattern(e.target.value)}
          placeholder="optional regex"
        />
      </div>
      <div className="grid gap-1">
        <Label htmlFor="rule-fop">{t("rule.filterOperationKeys")}</Label>
        <JsonField
          id="rule-fop"
          value={fopText}
          onChange={setFopText}
          rows={3}
          placeholder='["generate_content"]'
        />
      </div>
      <div className="grid gap-1">
        <Label>{t("rule.configJson")}</Label>
        <RuleConfigFields
          kind={kind}
          value={configValue}
          onChange={setConfigValue}
          onValidChange={setConfigValid}
        />
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

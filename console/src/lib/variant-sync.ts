import { api } from "@/api/http";
import { upsertRule, deleteRule, type Rule } from "@/api/rules";
import type { SuffixAction } from "@/components/providers/suffix-presets";
import {
  ensureProviderDefaultRuleSet,
  findProviderDefaultRuleSet,
} from "./provider-rule-set";

/** Suffix list from a model's `variants_json` (array form or {suffixes} object form). */
export function parseVariantSuffixes(variants: unknown): string[] {
  if (Array.isArray(variants)) return variants.map(String);
  if (variants && typeof variants === "object") {
    const o = variants as { suffixes?: unknown };
    if (Array.isArray(o.suffixes)) return o.suffixes.map(String);
  }
  return [];
}

export interface RuleDraft {
  kind: "rewrite";
  config_json: { path: string; action: "set"; value_json: unknown };
  filter_model_pattern: string;
  filter_operation_keys: null;
  sort_order: number;
  enabled: true;
}

export interface VariantRulePlan {
  toDelete: number[];
  toCreate: RuleDraft[];
}

interface PlanArgs {
  modelId: string;
  oldSuffixes: string[];
  newSuffixes: string[];
  presetActions: Map<string, SuffixAction[]>;
  existingRules: Rule[];
}

/** Pure: decide which dedicated-set rules to delete and create for variant changes.
 *  - Removed suffixes (old∖new): delete their rules (matched by filter_model_pattern).
 *  - Preset-added suffixes: delete any existing rules for that full id (idempotent
 *    overwrite), then create one rewrite rule per action. */
export function planVariantRuleChanges(a: PlanArgs): VariantRulePlan {
  const fullId = (suffix: string) => `${a.modelId}${suffix}`;
  const newSet = new Set(a.newSuffixes);
  const removed = a.oldSuffixes.filter((s) => !newSet.has(s));

  const rekeyed = new Set<string>([
    ...removed.map(fullId),
    ...[...a.presetActions.keys()].map(fullId),
  ]);
  const toDelete = a.existingRules
    .filter((r) => r.filter_model_pattern != null && rekeyed.has(r.filter_model_pattern))
    .map((r) => r.id);

  const toCreate: RuleDraft[] = [];
  for (const [suffix, actions] of a.presetActions) {
    if (!newSet.has(suffix)) continue; // only for suffixes that actually remain
    actions.forEach((act, i) => {
      toCreate.push({
        kind: "rewrite",
        config_json: { path: act.path, action: "set", value_json: act.value },
        filter_model_pattern: fullId(suffix),
        filter_operation_keys: null,
        sort_order: i,
        enabled: true,
      });
    });
  }
  return { toDelete, toCreate };
}

interface SyncArgs {
  providerId: number;
  providerName: string;
  modelId: string;
  oldSuffixes: string[];
  newSuffixes: string[];
  presetActions: Map<string, SuffixAction[]>;
}

/** Apply variant rule changes against the provider's dedicated rule set.
 *  No-op when there is nothing to add or remove (never creates an empty set).
 *
 *  Known limitation (single-user deploy): renaming a model_id or deleting a model
 *  leaves its variant rules orphaned here — rules are keyed by `${modelId}${suffix}`
 *  and are not re-keyed/cleaned on rename/delete. Harmless: orphan literal patterns
 *  match no live (suffix-stripped) request. */
export async function syncModelVariants(a: SyncArgs): Promise<void> {
  const newSet = new Set(a.newSuffixes);
  const removed = a.oldSuffixes.filter((s) => !newSet.has(s));
  if (a.presetActions.size === 0 && removed.length === 0) return;

  const rsId = a.presetActions.size > 0
    ? await ensureProviderDefaultRuleSet(a.providerId, a.providerName)
    : await findProviderDefaultRuleSet(a.providerId);
  if (rsId == null) return; // removal-only and no dedicated set exists → nothing to clean

  const existingRules = await api<Rule[]>(`/admin/rule-sets/${rsId}/rules`);
  const plan = planVariantRuleChanges({ ...a, existingRules });

  for (const id of plan.toDelete) await deleteRule(id);
  for (const d of plan.toCreate) {
    await upsertRule(rsId, { rule_set_id: rsId, ...d });
  }
}

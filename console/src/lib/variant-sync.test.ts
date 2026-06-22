import { describe, expect, it } from "vitest";
import { planVariantRuleChanges, parseVariantSuffixes, type RuleDraft } from "./variant-sync";
import type { Rule } from "@/api/rules";
import type { SuffixAction } from "@/components/providers/suffix-presets";

function rule(id: number, pattern: string): Rule {
  return {
    id, rule_set_id: 1, kind: "rewrite", config_json: {},
    filter_model_pattern: pattern, filter_operation_keys: null,
    sort_order: 0, enabled: true, created_at: 0, updated_at: 0,
  };
}
const empty = new Map<string, SuffixAction[]>();

describe("parseVariantSuffixes", () => {
  it("reads array and object forms, else empty", () => {
    expect(parseVariantSuffixes(["-a", "-b"])).toEqual(["-a", "-b"]);
    expect(parseVariantSuffixes({ expose_base: false, suffixes: ["-x"] })).toEqual(["-x"]);
    expect(parseVariantSuffixes(null)).toEqual([]);
    expect(parseVariantSuffixes("nope")).toEqual([]);
  });
});

describe("planVariantRuleChanges", () => {
  it("creates one rewrite rule per action for a preset-added suffix", () => {
    const presetActions = new Map<string, SuffixAction[]>([
      ["-thinking-high", [{ path: "thinking", value: { type: "enabled", budget_tokens: 32768 } }]],
    ]);
    const plan = planVariantRuleChanges({
      modelId: "claude-x", oldSuffixes: [], newSuffixes: ["-thinking-high"],
      presetActions, existingRules: [],
    });
    expect(plan.toDelete).toEqual([]);
    expect(plan.toCreate).toHaveLength(1);
    const draft = plan.toCreate[0] as RuleDraft;
    expect(draft.filter_model_pattern).toBe("claude-x-thinking-high");
    expect(draft.config_json).toEqual({ path: "thinking", action: "set", value_json: { type: "enabled", budget_tokens: 32768 } });
  });

  it("deletes rules of a removed suffix", () => {
    const plan = planVariantRuleChanges({
      modelId: "m", oldSuffixes: ["-a", "-b"], newSuffixes: ["-a"],
      presetActions: empty, existingRules: [rule(7, "m-b"), rule(8, "m-a")],
    });
    expect(plan.toDelete).toEqual([7]);
    expect(plan.toCreate).toEqual([]);
  });

  it("idempotently overwrites: deletes existing rules for a re-added preset suffix", () => {
    const presetActions = new Map<string, SuffixAction[]>([
      ["-fast", [{ path: "service_tier", value: "priority" }]],
    ]);
    const plan = planVariantRuleChanges({
      modelId: "g", oldSuffixes: ["-fast"], newSuffixes: ["-fast"],
      presetActions, existingRules: [rule(3, "g-fast")],
    });
    expect(plan.toDelete).toEqual([3]);
    expect(plan.toCreate).toHaveLength(1);
  });

  it("no changes when only plain suffixes and no removals", () => {
    const plan = planVariantRuleChanges({
      modelId: "m", oldSuffixes: ["-x"], newSuffixes: ["-x", "-y"],
      presetActions: empty, existingRules: [],
    });
    expect(plan).toEqual({ toDelete: [], toCreate: [] });
  });
});

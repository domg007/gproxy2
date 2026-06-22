import { describe, expect, it } from "vitest";
import { planVariantRuleChanges, parseVariantNames, type RuleDraft } from "./variant-sync";
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

describe("parseVariantNames", () => {
  it("reads bare array and object forms, else empty", () => {
    expect(parseVariantNames(["gpt-image-2", "gpt-5.5-thinking"])).toEqual(["gpt-image-2", "gpt-5.5-thinking"]);
    expect(parseVariantNames({ expose_base: false, variants: ["qwen-fast"] })).toEqual(["qwen-fast"]);
    expect(parseVariantNames(null)).toEqual([]);
    expect(parseVariantNames("nope")).toEqual([]);
  });
});

describe("planVariantRuleChanges", () => {
  it("creates one rewrite rule per action keyed by the full variant name", () => {
    const presetActions = new Map<string, SuffixAction[]>([
      ["gpt-image-2", [{ path: "tools", value: [{ type: "image_generation" }] }]],
    ]);
    const plan = planVariantRuleChanges({
      oldNames: [], newNames: ["gpt-image-2"], presetActions, existingRules: [],
    });
    expect(plan.toDelete).toEqual([]);
    expect(plan.toCreate).toHaveLength(1);
    const draft = plan.toCreate[0] as RuleDraft;
    expect(draft.filter_model_pattern).toBe("gpt-image-2");
    expect(draft.config_json).toEqual({ path: "tools", action: "set", value_json: [{ type: "image_generation" }] });
  });

  it("deletes rules of a removed name", () => {
    const plan = planVariantRuleChanges({
      oldNames: ["gpt-image-2", "gpt-fast"], newNames: ["gpt-fast"],
      presetActions: empty, existingRules: [rule(7, "gpt-image-2"), rule(8, "gpt-fast")],
    });
    expect(plan.toDelete).toEqual([7]);
    expect(plan.toCreate).toEqual([]);
  });

  it("idempotently overwrites: deletes existing rule for a re-set name", () => {
    const presetActions = new Map<string, SuffixAction[]>([
      ["gpt-fast", [{ path: "service_tier", value: "priority" }]],
    ]);
    const plan = planVariantRuleChanges({
      oldNames: ["gpt-fast"], newNames: ["gpt-fast"],
      presetActions, existingRules: [rule(3, "gpt-fast")],
    });
    expect(plan.toDelete).toEqual([3]);
    expect(plan.toCreate).toHaveLength(1);
  });

  it("leaves untouched names alone: no behavior set, no removal", () => {
    const plan = planVariantRuleChanges({
      oldNames: ["gpt-image-2"], newNames: ["gpt-image-2", "gpt-new"],
      presetActions: empty, existingRules: [rule(5, "gpt-image-2")],
    });
    expect(plan).toEqual({ toDelete: [], toCreate: [] });
  });
});

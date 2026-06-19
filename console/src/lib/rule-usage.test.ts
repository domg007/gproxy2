import { describe, expect, it } from "vitest";
import {
  groupRulesByKind, sortRulesForPipeline, computeRuleSetUsage,
} from "./rule-usage";
import type { Rule, ProviderRuleSet } from "@/api/rules";

function rule(o: Partial<Rule>): Rule {
  return {
    id: 1, rule_set_id: 1, kind: "rewrite", config_json: {},
    filter_model_pattern: null, filter_operation_keys: null,
    sort_order: 0, enabled: true, created_at: 0, updated_at: 0, ...o,
  };
}
function attach(o: Partial<ProviderRuleSet>): ProviderRuleSet {
  return { id: 1, provider_id: 1, rule_set_id: 1, sort_order: 0, enabled: true, created_at: 0, updated_at: 0, ...o };
}

describe("sortRulesForPipeline", () => {
  it("orders by kind rank before sort_order", () => {
    const out = sortRulesForPipeline([
      rule({ id: 1, kind: "header", sort_order: 0 }),
      rule({ id: 2, kind: "system_text", sort_order: 99 }),
      rule({ id: 3, kind: "rewrite", sort_order: 1 }),
    ]);
    expect(out.map((r) => r.kind)).toEqual(["system_text", "rewrite", "header"]);
  });
});

describe("groupRulesByKind", () => {
  it("groups in execution order and drops empty kinds", () => {
    const groups = groupRulesByKind([
      rule({ id: 1, kind: "sanitize" }),
      rule({ id: 2, kind: "system_text" }),
    ]);
    expect(groups.map((g) => g.kind)).toEqual(["system_text", "sanitize"]);
  });
});

describe("computeRuleSetUsage", () => {
  it("classifies private / shared / unused", () => {
    const atts = [
      attach({ id: 1, rule_set_id: 7, provider_id: 1 }),
      attach({ id: 2, rule_set_id: 7, provider_id: 2 }),
      attach({ id: 3, rule_set_id: 8, provider_id: 1 }),
    ];
    expect(computeRuleSetUsage(7, atts).scope).toBe("shared");
    expect(computeRuleSetUsage(7, atts).providerIds).toEqual([1, 2]);
    expect(computeRuleSetUsage(8, atts).scope).toBe("private");
    expect(computeRuleSetUsage(9, atts).scope).toBe("unused");
  });
});

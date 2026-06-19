import { describe, expect, it } from "vitest";
import { SANITIZE_TEMPLATES, templateToRuleInputs } from "./sanitize-templates";

describe("templateToRuleInputs", () => {
  it("maps each template line to a sanitize RuleInput with incrementing sort_order", () => {
    const tpl = SANITIZE_TEMPLATES[0];
    const inputs = templateToRuleInputs(tpl, 5, 10);
    expect(inputs).toHaveLength(tpl.rules.length);
    expect(inputs[0]).toMatchObject({ rule_set_id: 5, kind: "sanitize", sort_order: 10, enabled: true });
    expect((inputs[0].config_json as { pattern: string }).pattern).toBe(tpl.rules[0].pattern);
  });
});

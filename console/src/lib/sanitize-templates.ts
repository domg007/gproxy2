import type { RuleInput } from "@/api/rules";

export interface SanitizeTemplate {
  id: string;
  rules: { pattern: string; replacement: string }[];
}

// Word-boundary client-identity scrubs (ported from v1 sanitize presets).
export const SANITIZE_TEMPLATES: SanitizeTemplate[] = [
  { id: "aider", rules: [{ pattern: "\\bAider\\b", replacement: "The assistant" }] },
  { id: "cline", rules: [{ pattern: "\\bCline\\b", replacement: "Assistant" }] },
  { id: "continue", rules: [{ pattern: "\\bContinue\\b", replacement: "Assistant" }] },
  { id: "cursor", rules: [{ pattern: "\\bCursor\\b", replacement: "Assistant" }] },
];

export function templateToRuleInputs(tpl: SanitizeTemplate, ruleSetId: number, baseSortOrder: number): RuleInput[] {
  return tpl.rules.map((r, i) => ({
    rule_set_id: ruleSetId,
    kind: "sanitize",
    config_json: { pattern: r.pattern, replacement: r.replacement },
    sort_order: baseSortOrder + i,
    enabled: true,
  }));
}

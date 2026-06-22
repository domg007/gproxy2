import type { SuffixGroup } from "./index";

export const CLAUDE_GROUPS: SuffixGroup[] = [
  {
    key: "thinking",
    label: "Thinking",
    entries: [
      { suffix: "-thinking-none", label: "thinking: disabled", actions: [{ path: "thinking", value: { type: "disabled" } }] },
      { suffix: "-thinking-low", label: "thinking: low (1024 tokens)", actions: [{ path: "thinking", value: { type: "enabled", budget_tokens: 1024, display: "summarized" } }] },
      { suffix: "-thinking-medium", label: "thinking: medium (10240 tokens)", actions: [{ path: "thinking", value: { type: "enabled", budget_tokens: 10240, display: "summarized" } }] },
      { suffix: "-thinking-high", label: "thinking: high (32768 tokens)", actions: [{ path: "thinking", value: { type: "enabled", budget_tokens: 32768, display: "summarized" } }] },
      { suffix: "-thinking-adaptive", label: "thinking: adaptive", actions: [{ path: "thinking", value: { type: "adaptive", display: "summarized" } }] },
    ],
  },
  {
    key: "effort",
    label: "Effort",
    entries: [
      { suffix: "-effort-low", label: "effort: low", actions: [{ path: "output_config", value: { effort: "low" } }] },
      { suffix: "-effort-medium", label: "effort: medium", actions: [{ path: "output_config", value: { effort: "medium" } }] },
      { suffix: "-effort-high", label: "effort: high", actions: [{ path: "output_config", value: { effort: "high" } }] },
      { suffix: "-effort-xhigh", label: "effort: xhigh", actions: [{ path: "output_config", value: { effort: "xhigh" } }] },
      { suffix: "-effort-max", label: "effort: max", actions: [{ path: "output_config", value: { effort: "max" } }] },
    ],
  },
];

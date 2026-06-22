import type { SuffixGroup } from "./index";

export const OPENAI_RESPONSE_GROUPS: SuffixGroup[] = [
  {
    key: "thinking",
    label: "Reasoning",
    entries: [
      { suffix: "-thinking-none", label: "reasoning: none", actions: [{ path: "reasoning", value: { effort: "none" } }] },
      { suffix: "-thinking-low", label: "reasoning: low", actions: [{ path: "reasoning", value: { effort: "low" } }] },
      { suffix: "-thinking-medium", label: "reasoning: medium", actions: [{ path: "reasoning", value: { effort: "medium" } }] },
      { suffix: "-thinking-high", label: "reasoning: high", actions: [{ path: "reasoning", value: { effort: "high" } }] },
      { suffix: "-thinking-xhigh", label: "reasoning: xhigh", actions: [{ path: "reasoning", value: { effort: "xhigh" } }] },
    ],
  },
  {
    key: "tier",
    label: "Service Tier",
    entries: [
      { suffix: "-tier-auto", label: "service_tier: auto", actions: [{ path: "service_tier", value: "auto" }] },
      { suffix: "-tier-default", label: "service_tier: default", actions: [{ path: "service_tier", value: "default" }] },
      { suffix: "-tier-flex", label: "service_tier: flex", actions: [{ path: "service_tier", value: "flex" }] },
      { suffix: "-tier-scale", label: "service_tier: scale", actions: [{ path: "service_tier", value: "scale" }] },
      { suffix: "-tier-priority", label: "service_tier: priority", actions: [{ path: "service_tier", value: "priority" }] },
      { suffix: "-fast", label: "fast (= priority)", actions: [{ path: "service_tier", value: "priority" }] },
    ],
  },
  {
    key: "verbosity",
    label: "Verbosity",
    entries: [
      { suffix: "-effort-low", label: "verbosity: low", actions: [{ path: "text", value: { verbosity: "low" } }] },
      { suffix: "-effort-medium", label: "verbosity: medium", actions: [{ path: "text", value: { verbosity: "medium" } }] },
      { suffix: "-effort-high", label: "verbosity: high", actions: [{ path: "text", value: { verbosity: "high" } }] },
    ],
  },
  {
    key: "tool",
    label: "Forced Tool",
    entries: [
      { suffix: "-image-generate", label: "force image_generation (generate)", actions: [{ path: "tools", value: [{ type: "image_generation", action: "generate" }] }, { path: "tool_choice", value: { type: "image_generation" } }] },
      { suffix: "-image-edit", label: "force image_generation (edit)", actions: [{ path: "tools", value: [{ type: "image_generation", action: "edit" }] }, { path: "tool_choice", value: { type: "image_generation" } }] },
      { suffix: "-search", label: "force web_search_preview", actions: [{ path: "tools", value: [{ type: "web_search_preview" }] }, { path: "tool_choice", value: { type: "web_search_preview" } }] },
      { suffix: "-deep-research", label: "force deep_research", actions: [{ path: "tools", value: [{ type: "deep_research" }] }, { path: "tool_choice", value: { type: "deep_research" } }] },
    ],
  },
];

export const OPENAI_CHAT_GROUPS: SuffixGroup[] = [
  {
    key: "thinking",
    label: "Reasoning",
    entries: [
      { suffix: "-thinking-none", label: "reasoning_effort: none", actions: [{ path: "reasoning_effort", value: "none" }] },
      { suffix: "-thinking-low", label: "reasoning_effort: low", actions: [{ path: "reasoning_effort", value: "low" }] },
      { suffix: "-thinking-medium", label: "reasoning_effort: medium", actions: [{ path: "reasoning_effort", value: "medium" }] },
      { suffix: "-thinking-high", label: "reasoning_effort: high", actions: [{ path: "reasoning_effort", value: "high" }] },
      { suffix: "-thinking-xhigh", label: "reasoning_effort: xhigh", actions: [{ path: "reasoning_effort", value: "xhigh" }] },
    ],
  },
  {
    key: "tier",
    label: "Service Tier",
    entries: [
      { suffix: "-tier-auto", label: "service_tier: auto", actions: [{ path: "service_tier", value: "auto" }] },
      { suffix: "-tier-default", label: "service_tier: default", actions: [{ path: "service_tier", value: "default" }] },
      { suffix: "-tier-flex", label: "service_tier: flex", actions: [{ path: "service_tier", value: "flex" }] },
      { suffix: "-tier-scale", label: "service_tier: scale", actions: [{ path: "service_tier", value: "scale" }] },
      { suffix: "-tier-priority", label: "service_tier: priority", actions: [{ path: "service_tier", value: "priority" }] },
      { suffix: "-fast", label: "fast (= priority)", actions: [{ path: "service_tier", value: "priority" }] },
    ],
  },
  {
    key: "verbosity",
    label: "Verbosity",
    entries: [
      { suffix: "-effort-low", label: "verbosity: low", actions: [{ path: "verbosity", value: "low" }] },
      { suffix: "-effort-medium", label: "verbosity: medium", actions: [{ path: "verbosity", value: "medium" }] },
      { suffix: "-effort-high", label: "verbosity: high", actions: [{ path: "verbosity", value: "high" }] },
    ],
  },
];

import type { SuffixGroup } from "./index";

export const GEMINI_GROUPS: SuffixGroup[] = [
  {
    key: "thinking",
    label: "Thinking",
    entries: [
      { suffix: "-thinking-none", label: "thinkingLevel: MINIMAL", actions: [{ path: "thinkingConfig", value: { thinkingLevel: "MINIMAL" } }] },
      { suffix: "-thinking-low", label: "thinkingLevel: LOW", actions: [{ path: "thinkingConfig", value: { thinkingLevel: "LOW" } }] },
      { suffix: "-thinking-medium", label: "thinkingLevel: MEDIUM", actions: [{ path: "thinkingConfig", value: { thinkingLevel: "MEDIUM" } }] },
      { suffix: "-thinking-high", label: "thinkingLevel: HIGH", actions: [{ path: "thinkingConfig", value: { thinkingLevel: "HIGH" } }] },
    ],
  },
];

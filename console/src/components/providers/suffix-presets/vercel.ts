import type { SuffixGroup } from "./index";

const VIA = ["openai", "anthropic", "google", "vertex", "bedrock", "groq", "deepseek", "xai", "mistral", "cohere", "perplexity"] as const;

export const VERCEL_GATEWAY_SOURCE_GROUP: SuffixGroup = {
  key: "vercel_gateway_source",
  label: "Vercel Gateway Source",
  entries: VIA.map((v) => ({
    suffix: `-via-${v}`,
    label: `providerOptions.gateway.only: ${v}`,
    actions: [{ path: "providerOptions.gateway.only", value: [v] }],
  })),
};

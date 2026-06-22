import { Type, Database, Pencil, Wand2, Heading } from "lucide-react";
import type { LucideIcon } from "lucide-react";

export const RULE_KIND_META: Record<string, { icon: LucideIcon; descKey: string }> = {
  system_text: { icon: Type, descKey: "kindDesc.system_text" },
  cache_breakpoint: { icon: Database, descKey: "kindDesc.cache_breakpoint" },
  rewrite: { icon: Pencil, descKey: "kindDesc.rewrite" },
  transform: { icon: Wand2, descKey: "kindDesc.transform" },
  header: { icon: Heading, descKey: "kindDesc.header" },
};

/** One-line human summary of a rule's config for list/pipeline rows. */
export function summarizeRuleConfig(kind: string, config: unknown): string {
  const c = (config ?? {}) as Record<string, unknown>;
  switch (kind) {
    case "system_text":
      return `${c.position ?? "prepend"}: ${String(c.text ?? "").slice(0, 40)}`;
    case "cache_breakpoint":
      return `${c.target ?? "system"}${c.ttl ? ` · ${c.ttl}` : ""}`;
    case "rewrite":
      return `${c.action ?? "set"} ${c.path ?? ""}`;
    case "transform": {
      const locate = (c.locate ?? {}) as Record<string, unknown>;
      const target = locate.path
        ? `path ${locate.path}`
        : locate.match
          ? `match /${locate.match}/`
          : "locate";
      const actions = Array.isArray(c.actions) ? c.actions : [];
      const first = (actions[0] ?? {}) as Record<string, unknown>;
      const replacement = first.from ? `${first.from} -> ${first.with ?? first.to ?? ""}` : `${first.with ?? first.to ?? ""}`;
      return `${c.phase ?? "request"} ${target}${replacement ? ` -> ${replacement}` : ""}`;
    }
    case "header":
      return `${c.name ?? ""}: ${c.value ?? ""}`;
    default:
      return JSON.stringify(config).slice(0, 50);
  }
}

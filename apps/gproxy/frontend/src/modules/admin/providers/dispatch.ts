import { getChannelConfig } from "./channels/registry";
import { isCustomChannel } from "./constants";
import { normalizeChannel } from "./settings";
import type { DispatchMode, DispatchRuleDraft, TemplateRoute } from "./types";

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function nextRuleId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

export function createDefaultDispatchRule(): DispatchRuleDraft {
  return {
    id: nextRuleId(),
    srcOperation: "GenerateContent",
    srcProtocol: "OpenAi",
    mode: "transform",
    dstOperation: "GenerateContent",
    dstProtocol: "Claude"
  };
}

function buildDispatchRulesFromTemplate(routes: readonly TemplateRoute[]): DispatchRuleDraft[] {
  return routes.map(([srcOperation, srcProtocol, dstOperation, dstProtocol, explicitMode]) => {
    const mode =
      explicitMode ??
      (srcOperation === dstOperation && srcProtocol === dstProtocol ? "passthrough" : "transform");
    return {
      id: nextRuleId(),
      srcOperation,
      srcProtocol,
      mode,
      dstOperation,
      dstProtocol
    };
  });
}

export function defaultDispatchRulesForChannel(channel: string): DispatchRuleDraft[] {
  const normalized = normalizeChannel(channel);
  const builtinConfig = getChannelConfig(normalized);
  if (builtinConfig) {
    return buildDispatchRulesFromTemplate(builtinConfig.dispatchTemplateRoutes);
  }
  if (isCustomChannel(normalized)) {
    const custom = getChannelConfig("custom");
    if (custom) {
      return buildDispatchRulesFromTemplate(custom.dispatchTemplateRoutes);
    }
  }
  return [createDefaultDispatchRule()];
}

function toJsonObject(value: unknown): Record<string, unknown> | null {
  if (isObject(value)) {
    return value;
  }
  if (typeof value === "string") {
    try {
      const parsed = JSON.parse(value);
      return isObject(parsed) ? parsed : null;
    } catch {
      return null;
    }
  }
  return null;
}

function parseDispatchRules(value: unknown): DispatchRuleDraft[] {
  const root = toJsonObject(value);
  const rules = Array.isArray(root?.rules) ? root.rules : [];
  const drafts: DispatchRuleDraft[] = [];

  for (const item of rules) {
    if (!isObject(item)) {
      continue;
    }
    const route = isObject(item.route) ? item.route : null;
    const srcOperation = typeof route?.operation === "string" ? route.operation : "";
    const srcProtocol = typeof route?.protocol === "string" ? route.protocol : "";
    if (!srcOperation || !srcProtocol) {
      continue;
    }

    const implementation = item.implementation;
    let mode: DispatchMode = "unsupported";
    let dstOperation = srcOperation;
    let dstProtocol = srcProtocol;

    if (implementation === "Passthrough") {
      mode = "passthrough";
    } else if (implementation === "Local") {
      mode = "local";
    } else if (implementation === "Unsupported") {
      mode = "unsupported";
    } else if (isObject(implementation)) {
      const transform = isObject(implementation.TransformTo) ? implementation.TransformTo : null;
      const destination = isObject(transform?.destination) ? transform.destination : null;
      const op = typeof destination?.operation === "string" ? destination.operation : "";
      const proto = typeof destination?.protocol === "string" ? destination.protocol : "";
      if (op && proto) {
        mode = "transform";
        dstOperation = op;
        dstProtocol = proto;
      }
    }

    drafts.push({
      id: nextRuleId(),
      srcOperation,
      srcProtocol,
      mode,
      dstOperation,
      dstProtocol
    });
  }

  return drafts.length === 0 ? [createDefaultDispatchRule()] : drafts;
}

function isSingleGenericDefaultRule(rules: DispatchRuleDraft[]): boolean {
  if (rules.length !== 1) {
    return false;
  }
  const [rule] = rules;
  return (
    rule.srcOperation === "GenerateContent" &&
    rule.srcProtocol === "OpenAi" &&
    rule.mode === "transform" &&
    rule.dstOperation === "GenerateContent" &&
    rule.dstProtocol === "Claude"
  );
}

export function resolveProviderDispatchRules(
  channel: string,
  dispatchJson: unknown
): DispatchRuleDraft[] {
  const parsed = parseDispatchRules(dispatchJson);
  const root = toJsonObject(dispatchJson);
  const rawRules = Array.isArray(root?.rules) ? root.rules : [];
  if (rawRules.length === 0 || isSingleGenericDefaultRule(parsed)) {
    return defaultDispatchRulesForChannel(channel);
  }
  return parsed;
}

export function buildDispatchJson(rules: DispatchRuleDraft[]): Record<string, unknown> {
  return {
    rules: rules.map((rule) => {
      let implementation: unknown;
      if (rule.mode === "passthrough") {
        implementation = "Passthrough";
      } else if (rule.mode === "local") {
        implementation = "Local";
      } else if (rule.mode === "unsupported") {
        implementation = "Unsupported";
      } else {
        implementation = {
          TransformTo: {
            destination: {
              operation: rule.dstOperation,
              protocol: rule.dstProtocol
            }
          }
        };
      }

      return {
        route: {
          operation: rule.srcOperation,
          protocol: rule.srcProtocol
        },
        implementation
      };
    })
  };
}

export function normalizeDispatchRules(rules: DispatchRuleDraft[]): DispatchRuleDraft[] {
  if (rules.length === 0) {
    throw new Error("dispatch must contain at least one rule");
  }
  return rules.map((rule, index) => {
    const srcOperation = rule.srcOperation.trim();
    const srcProtocol = rule.srcProtocol.trim();
    const dstOperation = rule.dstOperation.trim();
    const dstProtocol = rule.dstProtocol.trim();

    if (!srcOperation || !srcProtocol) {
      throw new Error(`dispatch rule[${index}] missing src operation/protocol`);
    }
    if (rule.mode === "transform" && (!dstOperation || !dstProtocol)) {
      throw new Error(`dispatch rule[${index}] transform missing dst operation/protocol`);
    }

    return {
      ...rule,
      srcOperation,
      srcProtocol,
      dstOperation: dstOperation || srcOperation,
      dstProtocol: dstProtocol || srcProtocol
    };
  });
}

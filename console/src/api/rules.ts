import { queryOptions } from "@tanstack/react-query";
import { api } from "./http";

export interface RuleSet { id: number; name: string; enabled: boolean; description: string | null; created_at: number; updated_at: number; }
export interface RuleSetInput { id?: number | null; name: string; enabled: boolean; description?: string | null; }

export interface Rule { id: number; rule_set_id: number; kind: string; config_json: unknown; filter_model_pattern: string | null; filter_operation_keys: unknown; sort_order: number; enabled: boolean; created_at: number; updated_at: number; }
export interface RuleInput { id?: number | null; rule_set_id: number; kind: string; config_json: unknown; filter_model_pattern?: string | null; filter_operation_keys?: unknown; sort_order: number; enabled: boolean; }

export interface RoutingRule { id: number; provider_id: number; operation: string; kind: string; implementation: string; dest_operation: string | null; dest_kind: string | null; sort_order: number; enabled: boolean; created_at: number; updated_at: number; }
export interface RoutingRuleInput { id?: number | null; provider_id: number; operation: string; kind: string; implementation: string; dest_operation?: string | null; dest_kind?: string | null; sort_order: number; enabled: boolean; }

export interface ProviderRuleSet { id: number; provider_id: number; rule_set_id: number; sort_order: number; enabled: boolean; created_at: number; updated_at: number; }
export interface ProviderRuleSetInput { id?: number | null; provider_id: number; rule_set_id: number; sort_order: number; enabled: boolean; }

// Enum vocabularies (snake_case wire; sourced from protocol/operation.rs + transform/routing.rs).
export const OPERATIONS = ["list_models","get_model","count_tokens","generate_content","stream_generate_content","create_image","edit_image","create_embedding","compact_content","create_conversation"] as const;
export const KINDS = ["open_ai_responses","open_ai_chat_completions","claude_messages","gemini_generate_content","open_ai","claude","gemini"] as const;
export const IMPLEMENTATIONS = ["passthrough","transform_to","local","unsupported"] as const;
export const RULE_KINDS = ["system_text","rewrite","sanitize","cache_breakpoint","header"] as const;

export const ruleSetsQuery = queryOptions({ queryKey: ["rule-sets"], queryFn: () => api<RuleSet[]>("/admin/rule-sets") });
export const ruleSetQuery = (id: number) => queryOptions({ queryKey: ["rule-sets", id], queryFn: () => api<RuleSet>(`/admin/rule-sets/${id}`) });
export const rulesQuery = (rsId: number) => queryOptions({ queryKey: ["rule-sets", rsId, "rules"], queryFn: () => api<Rule[]>(`/admin/rule-sets/${rsId}/rules`) });
export interface EffectiveRoute { operation: string; kind: string; implementation: string; dest_operation: string | null; dest_kind: string | null; source: "default" | "override"; }

export const routingRulesQuery = (pid: number) => queryOptions({ queryKey: ["providers", pid, "routing-rules"], queryFn: () => api<RoutingRule[]>(`/admin/providers/${pid}/routing-rules`) });
export const effectiveRoutingQuery = (pid: number) => queryOptions({ queryKey: ["providers", pid, "routing-rules", "effective"], queryFn: () => api<EffectiveRoute[]>(`/admin/providers/${pid}/routing-rules/effective`) });
export const providerRuleSetsQuery = (pid: number) => queryOptions({ queryKey: ["providers", pid, "rule-sets"], queryFn: () => api<ProviderRuleSet[]>(`/admin/providers/${pid}/rule-sets`) });

export function upsertRuleSet(i: RuleSetInput) { return api<RuleSet>("/admin/rule-sets", { method: "POST", body: JSON.stringify(i) }); }
export function deleteRuleSet(id: number) { return api<void>(`/admin/rule-sets/${id}`, { method: "DELETE" }); }
export function upsertRule(rsId: number, i: RuleInput) { return api<Rule>(`/admin/rule-sets/${rsId}/rules`, { method: "POST", body: JSON.stringify(i) }); }
export function deleteRule(id: number) { return api<void>(`/admin/rules/${id}`, { method: "DELETE" }); }
export function upsertRoutingRule(pid: number, i: RoutingRuleInput) { return api<RoutingRule>(`/admin/providers/${pid}/routing-rules`, { method: "POST", body: JSON.stringify(i) }); }
export function deleteRoutingRule(id: number) { return api<void>(`/admin/routing-rules/${id}`, { method: "DELETE" }); }
export function upsertProviderRuleSet(pid: number, i: ProviderRuleSetInput) { return api<ProviderRuleSet>(`/admin/providers/${pid}/rule-sets`, { method: "POST", body: JSON.stringify(i) }); }
export function deleteProviderRuleSet(id: number) { return api<void>(`/admin/provider-rule-sets/${id}`, { method: "DELETE" }); }

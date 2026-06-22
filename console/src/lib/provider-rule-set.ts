import { api } from "@/api/http";
import {
  upsertRuleSet, upsertProviderRuleSet, deleteRuleSet, deleteProviderRuleSet,
  type RuleSet, type ProviderRuleSet,
} from "@/api/rules";

/** Stored in a rule set's `description` to mark it as a provider's dedicated set.
 *  Rule sets are global (no provider_id column) — this sentinel is how we claim one. */
export function providerDefaultSentinel(providerId: number): string {
  return `gproxy:provider-default:${providerId}`;
}

/** Find the provider's dedicated set among its attachments, matched by sentinel. */
async function findSet(
  providerId: number,
): Promise<{ rsId: number; attachmentId: number } | null> {
  const attachments = await api<ProviderRuleSet[]>(`/admin/providers/${providerId}/rule-sets`);
  if (attachments.length === 0) return null;
  const all = await api<RuleSet[]>(`/admin/rule-sets`);
  const sentinel = providerDefaultSentinel(providerId);
  const byId = new Map(all.map((rs) => [rs.id, rs]));
  for (const a of attachments) {
    if (byId.get(a.rule_set_id)?.description === sentinel) {
      return { rsId: a.rule_set_id, attachmentId: a.id };
    }
  }
  return null;
}

export async function findProviderDefaultRuleSet(providerId: number): Promise<number | null> {
  return (await findSet(providerId))?.rsId ?? null;
}

/** Idempotent: return the dedicated set's id, creating + attaching it if absent. */
export async function ensureProviderDefaultRuleSet(
  providerId: number,
  providerName: string,
): Promise<number> {
  const found = await findSet(providerId);
  if (found) return found.rsId;
  const rs = await upsertRuleSet({
    name: `${providerName} · defaults`,
    enabled: true,
    description: providerDefaultSentinel(providerId),
  });
  const attachments = await api<ProviderRuleSet[]>(`/admin/providers/${providerId}/rule-sets`);
  await upsertProviderRuleSet(providerId, {
    provider_id: providerId, rule_set_id: rs.id, sort_order: attachments.length, enabled: true,
  });
  return rs.id;
}

/** Remove the dedicated set (and its rules). Best-effort detach of the attachment. */
export async function deleteProviderDefaultRuleSet(providerId: number): Promise<void> {
  const found = await findSet(providerId);
  if (!found) return;
  await deleteRuleSet(found.rsId);
  try {
    await deleteProviderRuleSet(found.attachmentId);
  } catch {
    // attachment likely cascade-removed with the rule set; ignore.
  }
}

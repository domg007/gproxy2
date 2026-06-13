import { queryOptions } from "@tanstack/react-query";
import { api } from "./http";

export interface ProviderModel {
  id: number;
  provider_id: number;
  model_id: string;
  display_name: string | null;
  pricing_json: unknown;
  variants_json: unknown;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface ProviderModelInput {
  id?: number | null;
  provider_id: number;
  model_id: string;
  display_name?: string | null;
  /** OMIT when empty — JSON null round-trips as Some(Value::Null). */
  pricing_json?: unknown;
  variants_json?: unknown;
  enabled: boolean;
}

export const providerModelsQuery = (providerId: number) =>
  queryOptions({
    queryKey: ["providers", providerId, "models"],
    queryFn: () => api<ProviderModel[]>(`/admin/providers/${providerId}/models`),
  });

export function upsertProviderModel(providerId: number, input: ProviderModelInput): Promise<ProviderModel> {
  return api<ProviderModel>(`/admin/providers/${providerId}/models`, {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function deleteProviderModel(id: number): Promise<void> {
  return api<void>(`/admin/provider-models/${id}`, { method: "DELETE" });
}

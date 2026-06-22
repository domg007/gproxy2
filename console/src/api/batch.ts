import { api } from "./http";

export type BatchOp = "enable" | "disable" | "delete";

export interface BatchOutcome {
  affected: number;
  errors: { id: number; message: string }[];
}

/** POST /admin/batch/{entity} — 批量启用/禁用/删除。 */
export function batchOp(
  entity: string,
  op: BatchOp,
  ids: (number | string)[],
): Promise<BatchOutcome> {
  return api<BatchOutcome>(`/admin/batch/${entity}`, {
    method: "POST",
    body: JSON.stringify({ op, ids: ids.map(Number) }),
  });
}

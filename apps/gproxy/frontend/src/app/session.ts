import { ApiError, apiRequest } from "../lib/api";
import type { UserRole } from "../lib/types";

export interface SessionInfo {
  role: UserRole;
}

export async function detectRole(apiKey: string): Promise<SessionInfo> {
  try {
    await apiRequest("/admin/global-settings", { apiKey, method: "GET" });
    return { role: "admin" };
  } catch (error) {
    if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
      await apiRequest("/user/keys/query", { apiKey, method: "POST" });
      return { role: "user" };
    }
    throw error;
  }
}

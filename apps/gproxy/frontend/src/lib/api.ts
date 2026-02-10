export type RequestOptions = {
  method?: "GET" | "POST" | "PUT" | "DELETE";
  body?: unknown;
  adminKey?: string;
  userKey?: string;
  query?: Record<string, string | number | boolean | undefined | null>;
};

export class ApiError extends Error {
  status: number;
  detail?: string;

  constructor(status: number, message: string, detail?: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.detail = detail;
  }
}

function withQuery(path: string, query?: RequestOptions["query"]): string {
  if (!query) {
    return path;
  }
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === null || value === undefined || value === "") {
      continue;
    }
    params.set(key, String(value));
  }
  const qs = params.toString();
  if (!qs) {
    return path;
  }
  return `${path}?${qs}`;
}

function authHeaders(adminKey?: string, userKey?: string): Headers {
  const headers = new Headers();
  headers.set("Accept", "application/json");
  if (adminKey) {
    headers.set("x-admin-key", adminKey);
  }
  if (userKey) {
    headers.set("Authorization", `Bearer ${userKey}`);
  }
  return headers;
}

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers = authHeaders(options.adminKey, options.userKey);
  let body: string | undefined;
  if (options.body !== undefined) {
    headers.set("Content-Type", "application/json");
    body = JSON.stringify(options.body);
  }

  const response = await fetch(withQuery(path, options.query), {
    method: options.method ?? "GET",
    headers,
    body
  });

  const text = await response.text();
  const parsed = text ? safeParseJson(text) : null;

  if (!response.ok) {
    if (parsed && typeof parsed === "object") {
      const source = parsed as Record<string, unknown>;
      const message =
        (typeof source.error === "string" && source.error) ||
        (typeof source.message === "string" && source.message) ||
        `HTTP ${response.status}`;
      const detail = typeof source.detail === "string" ? source.detail : undefined;
      throw new ApiError(response.status, message, detail);
    }
    throw new ApiError(response.status, text || `HTTP ${response.status}`);
  }

  return (parsed ?? (undefined as T)) as T;
}

export function safeParseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

export function formatApiError(error: unknown): string {
  if (error instanceof ApiError) {
    return error.detail ? `${error.message}: ${error.detail}` : error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error ?? "Unknown error");
}

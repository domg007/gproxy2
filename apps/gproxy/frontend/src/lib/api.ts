import type { ApiErrorData } from "./types";

export class ApiError extends Error {
  status: number;

  constructor(data: ApiErrorData) {
    super(data.message);
    this.status = data.status;
    this.name = "ApiError";
  }
}

interface RequestOptions {
  apiKey: string;
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  body?: unknown;
}

export async function apiRequest<T>(
  path: string,
  { apiKey, method = "GET", body }: RequestOptions
): Promise<T> {
  const headers = new Headers();
  headers.set("x-api-key", apiKey);

  let payload: string | undefined;
  if (body !== undefined) {
    headers.set("content-type", "application/json");
    payload = JSON.stringify(body);
  }

  const res = await fetch(path, {
    method,
    headers,
    body: payload
  });

  const text = await res.text();
  const maybeJson = parseMaybeJson(text);

  if (!res.ok) {
    throw new ApiError({
      status: res.status,
      message: extractErrorMessage(maybeJson, text, res.status)
    });
  }

  return maybeJson as T;
}

export function parseMaybeJson(text: string): unknown {
  const trimmed = text.trim();
  if (!trimmed) {
    return {};
  }
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) {
    return trimmed;
  }
  try {
    return JSON.parse(trimmed);
  } catch {
    return trimmed;
  }
}

function extractErrorMessage(payload: unknown, raw: string, status: number): string {
  if (payload && typeof payload === "object") {
    const object = payload as Record<string, unknown>;
    if (typeof object.error === "string") {
      return object.error;
    }
    if (typeof object.message === "string") {
      return object.message;
    }
  }
  if (raw.trim()) {
    return raw.trim();
  }
  return `request failed (${status})`;
}

export function isApiError(error: unknown): error is ApiError {
  return error instanceof ApiError;
}

export function formatError(error: unknown): string {
  if (isApiError(error)) {
    return `HTTP ${error.status}: ${error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

export function parseJsonObject(value: string, field: string): Record<string, unknown> {
  const trimmed = value.trim();
  if (!trimmed) {
    return {};
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (error) {
    throw new Error(`${field} is not valid JSON: ${String(error)}`);
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`${field} must be a JSON object`);
  }
  return parsed as Record<string, unknown>;
}

export function parseOptionalJsonObject(
  value: string,
  field: string
): Record<string, unknown> | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  return parseJsonObject(trimmed, field);
}

export function parseRequiredI64(value: string, field: string): number {
    const parsed = Number(value);
    if (!Number.isInteger(parsed)) {
        throw new Error(`${field} must be an integer`);
    }
    return parsed;
}

export function parseRequiredPositiveInteger(value: string, field: string): number {
  const parsed = Number(value);
  if (!Number.isInteger(parsed)) {
    throw new Error(`${field} must be an integer`);
  }
  if (parsed < 1) {
    throw new Error(`${field} must be at least 1`);
  }
  return parsed;
}

export function parseOptionalI64(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed)) {
    throw new Error("must be an integer");
  }
  return parsed;
}

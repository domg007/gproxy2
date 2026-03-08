import type { CredentialQueryRow } from "../../../lib/types";
import { getChannelConfig } from "./channels/registry";
import { normalizeChannel } from "./settings";
import type {
  ChannelCredentialSchema,
  CredentialFieldSchema,
  CredentialFieldValue,
  CredentialFormState
} from "./types";

function asObject(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function defaultFieldValue(field: CredentialFieldSchema): CredentialFieldValue {
  if (field.type === "boolean") {
    return false;
  }
  if (field.type === "optional_boolean") {
    return null;
  }
  return "";
}

function decodeSecretPayload(
  schema: ChannelCredentialSchema,
  secretJson: Record<string, unknown>
): Record<string, unknown> {
  if (schema.wrapper === "Custom") {
    return asObject(secretJson.Custom) ?? secretJson;
  }
  const builtins = asObject(secretJson.Builtin);
  if (!builtins || !schema.builtinVariant) {
    return {};
  }
  return asObject(builtins[schema.builtinVariant]) ?? {};
}

function secretValuesFromPayload(
  schema: ChannelCredentialSchema,
  defaults: Record<string, CredentialFieldValue>,
  payload: Record<string, unknown>
): Record<string, CredentialFieldValue> {
  const secretValues: Record<string, CredentialFieldValue> = { ...defaults };

  for (const field of schema.fields) {
    const raw = payload[field.key];
    if (field.type === "boolean") {
      secretValues[field.key] = raw === true;
      continue;
    }
    if (field.type === "optional_boolean") {
      if (raw === true || raw === false) {
        secretValues[field.key] = raw;
      } else {
        secretValues[field.key] = null;
      }
      continue;
    }
    if (field.type === "integer") {
      if (typeof raw === "number" && Number.isFinite(raw)) {
        secretValues[field.key] = String(Math.trunc(raw));
      } else if (typeof raw === "string") {
        secretValues[field.key] = raw;
      } else {
        secretValues[field.key] = "";
      }
      continue;
    }
    if (typeof raw === "string") {
      secretValues[field.key] = raw;
    } else {
      secretValues[field.key] = "";
    }
  }

  return secretValues;
}

function encodeCredentialPayload(
  schema: ChannelCredentialSchema,
  payload: Record<string, unknown>
): Record<string, unknown> {
  if (schema.wrapper === "Custom") {
    return { Custom: payload };
  }
  return {
    Builtin: {
      [schema.builtinVariant ?? "OpenAi"]: payload
    }
  };
}

function unquote(value: string): string {
  if (value.length < 2) {
    return value;
  }
  const first = value[0];
  const last = value[value.length - 1];
  if ((first === "\"" && last === "\"") || (first === "'" && last === "'")) {
    return value.slice(1, -1).trim();
  }
  return value;
}

function normalizeClaudeCodeCookie(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) {
    return "";
  }
  const sessionKeyMatch = trimmed.match(/(?:^|[;\s])sessionKey\s*=\s*([^;]+)/i);
  if (sessionKeyMatch) {
    return unquote(sessionKeyMatch[1]?.trim() ?? "");
  }
  const unquoted = unquote(trimmed);
  const keyMatch = unquoted.match(/(sk-ant-[A-Za-z0-9_-]+)/);
  if (keyMatch) {
    return keyMatch[1];
  }
  return unquoted;
}

function normalizedStringValue(
  values: Record<string, CredentialFieldValue>,
  key: string
): string {
  const value = values[key];
  if (typeof value !== "string") {
    return "";
  }
  return value.trim();
}

type CookiePair = {
  key: string;
  value: string;
  text: string;
};

function parseCookiePairs(raw: string): CookiePair[] {
  return raw
    .split(";")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const separatorIndex = part.indexOf("=");
      if (separatorIndex < 0) {
        return null;
      }
      const key = part.slice(0, separatorIndex).trim();
      if (!key) {
        return null;
      }
      const value = part.slice(separatorIndex + 1).trim();
      return {
        key,
        value,
        text: `${key}=${value}`
      };
    })
    .filter((pair): pair is CookiePair => pair !== null);
}

function isLikelyGrokCookieHeader(raw: string): boolean {
  const lowered = raw.trim().toLowerCase();
  if (!lowered) {
    return false;
  }
  return lowered.includes("sso=") || lowered.includes("sso-rw=") || lowered.includes(";");
}

export function parseGrokCookieHeader(raw: string): {
  sso: string;
} | null {
  const trimmed = raw.trim();
  if (!isLikelyGrokCookieHeader(trimmed)) {
    return null;
  }

  const pairs = parseCookiePairs(trimmed);
  if (pairs.length === 0) {
    return null;
  }

  let sso = "";

  for (const pair of pairs) {
    const lowered = pair.key.toLowerCase();
    if (lowered === "sso" || lowered === "sso-rw") {
      if (!sso && pair.value) {
        sso = pair.value;
      }
    }
  }

  if (!sso) {
    return null;
  }

  return { sso };
}

export function parseGrokSecretValuesFromCookieHeader(
  raw: string
): Record<string, CredentialFieldValue> {
  const base = createEmptyCredentialFormState("grok-web");
  const parsed = parseGrokCookieHeader(raw);
  if (parsed?.sso) {
    return {
      ...base.secretValues,
      sso: parsed.sso
    };
  }
  return {
    ...base.secretValues,
    sso: raw.trim()
  };
}

export function buildGrokCookieHeaderFromSecretValues(
  values: Record<string, CredentialFieldValue>
): string {
  const sso = normalizedStringValue(values, "sso");
  const parts: string[] = [];

  if (sso) {
    parts.push(`sso=${sso}`, `sso-rw=${sso}`);
  }

  return parts.join("; ");
}

function firstChars(value: string, length: number): string {
  if (!value) {
    return "";
  }
  return value.slice(0, Math.max(1, length));
}

export function credentialSchemaForChannel(channel: string): ChannelCredentialSchema {
  const normalized = normalizeChannel(channel);
  const config = getChannelConfig(normalized) ?? getChannelConfig("custom");
  if (!config) {
    throw new Error("missing custom channel credential schema");
  }
  return config.credentialSchema;
}

export function createEmptyCredentialFormState(channel: string): CredentialFormState {
  const schema = credentialSchemaForChannel(channel);
  const secretValues: Record<string, CredentialFieldValue> = {};
  for (const field of schema.fields) {
    secretValues[field.key] = defaultFieldValue(field);
  }
  return {
    id: "",
    name: "",
    kind: schema.kind,
    secretValues,
    settingsPayload: null,
    enabled: true
  };
}

export function credentialFormFromRow(
  channel: string,
  row: CredentialQueryRow
): CredentialFormState {
  const schema = credentialSchemaForChannel(channel);
  const base = createEmptyCredentialFormState(channel);
  const payload = decodeSecretPayload(schema, row.secret_json);
  const secretValues = secretValuesFromPayload(schema, base.secretValues, payload);

  return {
    id: String(row.id),
    name: row.name ?? "",
    kind: row.kind || schema.kind,
    secretValues,
    settingsPayload: row.settings_json ?? null,
    enabled: row.enabled
  };
}

export function secretValuesFromSecretJson(
  channel: string,
  secretJson: Record<string, unknown>
): Record<string, CredentialFieldValue> {
  const schema = credentialSchemaForChannel(channel);
  const base = createEmptyCredentialFormState(channel);
  const payload = decodeSecretPayload(schema, secretJson);
  return secretValuesFromPayload(schema, base.secretValues, payload);
}

export function buildCredentialSecretJson(
  channel: string,
  values: Record<string, CredentialFieldValue>
): string {
  const schema = credentialSchemaForChannel(channel);
  const normalizedChannel = normalizeChannel(channel);
  const payload: Record<string, unknown> = {};

  if (normalizedChannel === "grok-web") {
    const rawSsoInput = normalizedStringValue(values, "sso");
    const parsedFromCookie = parseGrokCookieHeader(rawSsoInput);
    payload.sso = parsedFromCookie?.sso ? parsedFromCookie.sso : rawSsoInput;

    return JSON.stringify(encodeCredentialPayload(schema, payload));
  }

  for (const field of schema.fields) {
    const raw = values[field.key];
    if (field.type === "boolean") {
      payload[field.key] = raw === true;
      continue;
    }
    if (field.type === "optional_boolean") {
      if (raw === true || raw === false) {
        payload[field.key] = raw;
      }
      continue;
    }
    const rawText = typeof raw === "string" ? raw.trim() : "";
    const text =
      normalizedChannel === "claudecode" && field.key === "cookie"
        ? normalizeClaudeCodeCookie(rawText)
        : rawText;
    if (field.type === "optional_string") {
      if (text) {
        payload[field.key] = text;
      }
      continue;
    }
    if (field.type === "integer") {
      const source = text || "0";
      const parsed = Number(source);
      if (!Number.isInteger(parsed)) {
        throw new Error(`invalid integer field: ${field.key}`);
      }
      payload[field.key] = parsed;
      continue;
    }
    payload[field.key] = text;
  }

  return JSON.stringify(encodeCredentialPayload(schema, payload));
}

export function credentialDefaultNameFromSecretValues(
  channel: string,
  values: Record<string, CredentialFieldValue>
): string | null {
  const normalizedChannel = normalizeChannel(channel);

  const userEmail = normalizedStringValue(values, "user_email");
  if (userEmail) {
    return userEmail;
  }

  const apiKey = normalizedStringValue(values, "api_key");
  if (apiKey) {
    return firstChars(apiKey, 16);
  }

  if (normalizedChannel === "claudecode") {
    const cookie = normalizedStringValue(values, "cookie");
    if (cookie) {
      return firstChars(normalizeClaudeCodeCookie(cookie), 12);
    }
  }

  return null;
}

export function credentialDefaultNameFromSecretJson(
  channel: string,
  secretJson: Record<string, unknown>
): string | null {
  const values = secretValuesFromSecretJson(channel, secretJson);
  return credentialDefaultNameFromSecretValues(channel, values);
}

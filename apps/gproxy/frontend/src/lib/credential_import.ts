import { buildCredentialSecret, credentialFieldMap, extractCredentialFields } from "./provider_schema";
import type { ProviderKind } from "./types";

export type ImportedCredential = {
  name: string | null;
  secretJson: Record<string, unknown>;
};

const JSON_CREDENTIAL_KINDS = new Set<ProviderKind>([
  "vertex",
  "claudecode",
  "antigravity",
  "geminicli",
  "codex"
]);

const PLACEHOLDER_PREFIXES = ["<required", "<optional", "<必填", "<可选"];

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function splitJsonDocuments(input: string): string[] {
  const documents: string[] = [];
  const text = input.trim();
  if (!text) {
    return documents;
  }

  let start = -1;
  let depth = 0;
  let inString = false;
  let escaped = false;

  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (start === -1) {
      if (/\s/.test(ch)) {
        continue;
      }
      if (ch === "{" || ch === "[") {
        start = i;
        depth = 1;
        continue;
      }
      throw new Error("Invalid JSON document");
    }

    if (inString) {
      if (escaped) {
        escaped = false;
        continue;
      }
      if (ch === "\\") {
        escaped = true;
        continue;
      }
      if (ch === "\"") {
        inString = false;
      }
      continue;
    }

    if (ch === "\"") {
      inString = true;
      continue;
    }
    if (ch === "{" || ch === "[") {
      depth += 1;
      continue;
    }
    if (ch === "}" || ch === "]") {
      depth -= 1;
      if (depth < 0) {
        throw new Error("Invalid JSON document");
      }
      if (depth === 0) {
        documents.push(text.slice(start, i + 1));
        start = -1;
      }
    }
  }

  if (start !== -1 || depth !== 0 || inString) {
    throw new Error("Invalid JSON document");
  }

  return documents;
}

function flattenJsonDocuments(documents: string[]): Record<string, unknown>[] {
  const rows: Record<string, unknown>[] = [];
  for (const document of documents) {
    const parsed = JSON.parse(document) as unknown;
    if (Array.isArray(parsed)) {
      for (const item of parsed) {
        if (isRecord(item)) {
          rows.push(item);
        }
      }
      continue;
    }
    if (isRecord(parsed)) {
      rows.push(parsed);
    }
  }
  return rows;
}

function deriveJsonName(fields: Record<string, string>): string | null {
  const email = [fields.user_email, fields.client_email, fields.email]
    .map((value) => value?.trim() ?? "")
    .find((value) => value && !PLACEHOLDER_PREFIXES.some((prefix) => value.startsWith(prefix))) ?? "";
  return email || null;
}

function normalizeFieldValue(value: unknown): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  const text = String(value).trim();
  if (!text) {
    return null;
  }
  if (PLACEHOLDER_PREFIXES.some((prefix) => text.startsWith(prefix))) {
    return null;
  }
  return text;
}

export function isJsonCredentialKind(kind: ProviderKind): boolean {
  return JSON_CREDENTIAL_KINDS.has(kind);
}

export function parseJsonCredentialText(text: string): Record<string, unknown>[] {
  if (!text.trim()) {
    return [];
  }
  const documents = splitJsonDocuments(text);
  return flattenJsonDocuments(documents);
}

export function buildImportedCredentialFromKey(
  kind: ProviderKind,
  key: string
): ImportedCredential {
  const trimmed = key.trim();
  return {
    name: trimmed.slice(0, 16) || null,
    secretJson: buildCredentialSecret(kind, { api_key: trimmed })
  };
}

export function buildImportedCredentialFromJson(
  kind: ProviderKind,
  raw: Record<string, unknown>
): ImportedCredential | null {
  const wrapped = extractCredentialFields(kind, raw);
  const wrappedFields: Record<string, string> = {};
  for (const [key, value] of Object.entries(wrapped)) {
    const normalized = normalizeFieldValue(value);
    if (normalized !== null) {
      wrappedFields[key] = normalized;
    }
  }
  if (Object.keys(wrappedFields).length > 0) {
    return {
      name: deriveJsonName(wrappedFields),
      secretJson: raw
    };
  }

  const fields: Record<string, string> = {};
  for (const spec of credentialFieldMap[kind]) {
    const value = raw[spec.key];
    const normalized = normalizeFieldValue(value);
    if (normalized === null) {
      continue;
    }
    fields[spec.key] = normalized;
  }
  if (Object.keys(fields).length === 0) {
    return null;
  }

  return {
    name: deriveJsonName(fields),
    secretJson: buildCredentialSecret(kind, fields)
  };
}

export function buildJsonCredentialTemplate(kind: ProviderKind): string {
  const template: Record<string, string> = {};
  for (const spec of credentialFieldMap[kind]) {
    const suffix = spec.type === "number" ? ":number" : spec.type === "boolean" ? ":boolean" : "";
    template[spec.key] = spec.required ? `<required${suffix}>` : `<optional${suffix}>`;
  }
  return JSON.stringify(template, null, 2);
}

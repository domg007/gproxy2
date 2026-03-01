export type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export type CredentialHealthKind = "healthy" | "partial" | "dead";

export type CooldownItem = {
  model: string;
  untilUnixMs: number;
};

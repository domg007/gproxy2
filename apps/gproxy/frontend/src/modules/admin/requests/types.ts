import type {
  DownstreamRequestQueryRow,
  UpstreamRequestQueryRow
} from "../../../lib/types";

export type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
export type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export type SelectOption = {
  value: string;
  label: string;
};

export type RequestKind = "upstream" | "downstream";
export type RequestRow = UpstreamRequestQueryRow | DownstreamRequestQueryRow;

export type RequestQuerySnapshot = {
  kind: RequestKind;
  providerId: number | null;
  credentialId: number | null;
  userId: number | null;
  userKeyId: number | null;
  pathContains: string;
  fromUnixMs: number | null;
  toUnixMs: number | null;
  maxRows: number | null;
};

export type RequestBodyPayload = {
  request_body: number[] | null;
  response_body: number[] | null;
};

export type PayloadPreview = {
  preview: string;
  full: string;
  truncated: boolean;
};

export type RequestsFilterState = {
  providerId: string;
  credentialId: string;
  userId: string;
  userKeyId: string;
  requestPathContains: string;
  fromAt: string;
  toAt: string;
  limit: string;
};

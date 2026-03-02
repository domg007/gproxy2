import { parseAtToUnixMs } from "../../../lib/datetime";

export type LiveUsageRow = {
  name: string;
  percent: number | null;
  resetAt: string | number | null;
};

export type UsageWindow =
  | "5h"
  | "1d"
  | "1w"
  | "sum"
  | "primary"
  | "secondary"
  | "code_review";

export type UsageWindowSpec = {
  window: UsageWindow;
  fromUnixMs: number;
  toUnixMs: number;
};

export type UsageDisplayKind = "calls" | "tokens";

export type UsageDisplayRow = {
  label: string;
  window: UsageWindow;
  fromUnixMs: number;
  toUnixMs: number;
  calls: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  cacheCreationTokens5m: number;
  cacheCreationTokens1h: number;
  cacheTokens: number;
  totalTokens: number;
};

export type UsageSampleRow = {
  at: string;
  model: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_read_input_tokens: number | null;
  cache_creation_input_tokens: number | null;
  cache_creation_input_tokens_5min: number | null;
  cache_creation_input_tokens_1h: number | null;
};

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;
const WEEK_MS = 7 * DAY_MS;

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function asNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) {
      return null;
    }
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function toUsagePercent(value: unknown, mode: "used_percent" | "remaining_fraction"): number | null {
  const raw = asNumber(value);
  if (raw === null) {
    return null;
  }
  if (mode === "remaining_fraction") {
    return raw <= 1 ? (1 - raw) * 100 : 100 - raw;
  }
  return raw;
}

function toResetAt(value: unknown): string | number | null {
  if (typeof value === "string" && value.trim()) {
    return value.trim();
  }
  const raw = asNumber(value);
  if (raw === null) {
    return null;
  }
  return raw < 1_000_000_000_000 ? raw * 1000 : raw;
}

function pushLiveRow(
  rows: LiveUsageRow[],
  name: string,
  percent: number | null,
  resetAt: string | number | null
) {
  if (percent === null && resetAt === null) {
    return;
  }
  rows.push({ name, percent, resetAt });
}

function normalizeModelLabel(value: string): string {
  const trimmed = value.trim().replace(/^\/+/, "");
  if (!trimmed) {
    return "";
  }
  if (trimmed.startsWith("models/")) {
    return trimmed.slice("models/".length).trim();
  }
  return trimmed;
}

function parseCodexUsage(payload: Record<string, unknown>): LiveUsageRow[] {
  const rows: LiveUsageRow[] = [];

  const rateLimit = asRecord(payload.rate_limit);
  if (rateLimit) {
    const primary = asRecord(rateLimit.primary_window);
    const secondary = asRecord(rateLimit.secondary_window);
    if (primary) {
      pushLiveRow(
        rows,
        "primary",
        toUsagePercent(primary.used_percent, "used_percent"),
        toResetAt(primary.reset_at ?? primary.resetAt)
      );
    }
    if (secondary) {
      pushLiveRow(
        rows,
        "secondary",
        toUsagePercent(secondary.used_percent, "used_percent"),
        toResetAt(secondary.reset_at ?? secondary.resetAt)
      );
    }
  }

  const codeReview = asRecord(payload.code_review_rate_limit);
  if (codeReview) {
    const codeReviewWindow =
      asRecord(codeReview.primary_window) ?? asRecord(codeReview.secondary_window);
    if (codeReviewWindow) {
      pushLiveRow(
        rows,
        "code_review",
        toUsagePercent(codeReviewWindow.used_percent, "used_percent"),
        toResetAt(codeReviewWindow.reset_at ?? codeReviewWindow.resetAt)
      );
    }
  }

  return rows;
}

function parseClaudeCodeUsage(payload: Record<string, unknown>): LiveUsageRow[] {
  const rows: LiveUsageRow[] = [];
  for (const [name, value] of Object.entries(payload)) {
    const section = asRecord(value);
    if (!section) {
      continue;
    }
    pushLiveRow(
      rows,
      name,
      toUsagePercent(section.utilization, "used_percent"),
      toResetAt(section.resets_at ?? section.resetAt)
    );
  }
  return rows;
}

function parseGeminiCliUsage(payload: Record<string, unknown>): LiveUsageRow[] {
  const rows: LiveUsageRow[] = [];
  const buckets = Array.isArray(payload.buckets) ? payload.buckets : [];
  for (const item of buckets) {
    const bucket = asRecord(item);
    if (!bucket) {
      continue;
    }
    const modelIdRaw = typeof bucket.modelId === "string" ? bucket.modelId : "unknown";
    const modelId = normalizeModelLabel(modelIdRaw) || "unknown";
    const tokenType = typeof bucket.tokenType === "string" ? bucket.tokenType : "";
    const name = tokenType ? `${modelId} (${tokenType})` : modelId;
    pushLiveRow(
      rows,
      name,
      toUsagePercent(bucket.remainingFraction, "remaining_fraction"),
      toResetAt(bucket.resetTime)
    );
  }
  return rows;
}

function parseAntigravityUsage(payload: Record<string, unknown>): LiveUsageRow[] {
  const rows: LiveUsageRow[] = [];
  const models = asRecord(payload.models);
  if (!models) {
    return rows;
  }
  for (const [modelId, raw] of Object.entries(models)) {
    const model = asRecord(raw);
    if (!model) {
      continue;
    }
    const quota = asRecord(model.quotaInfo);
    pushLiveRow(
      rows,
      normalizeModelLabel(modelId) || modelId,
      toUsagePercent(quota?.remainingFraction, "remaining_fraction"),
      toResetAt(quota?.resetTime)
    );
  }
  return rows;
}

export function parseLiveUsageRows(channel: string, payload: unknown): LiveUsageRow[] {
  const root = asRecord(payload);
  if (!root) {
    return [];
  }
  const normalized = channel.trim().toLowerCase();
  if (normalized === "codex") {
    return parseCodexUsage(root);
  }
  if (normalized === "claudecode") {
    return parseClaudeCodeUsage(root);
  }
  if (normalized === "geminicli") {
    return parseGeminiCliUsage(root);
  }
  if (normalized === "antigravity") {
    return parseAntigravityUsage(root);
  }
  return [];
}

function parseResetMs(value: string | number | null): number | null {
  if (value === null) {
    return null;
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  if (/^-?\d+$/.test(trimmed)) {
    const numeric = Number(trimmed);
    if (!Number.isFinite(numeric)) {
      return null;
    }
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const date = new Date(trimmed);
  return Number.isNaN(date.getTime()) ? null : date.getTime();
}

function nearestFutureMs(values: Array<number | null>, nowMs: number): number | null {
  let out: number | null = null;
  for (const value of values) {
    if (value === null || value <= nowMs) {
      continue;
    }
    if (out === null || value < out) {
      out = value;
    }
  }
  return out;
}

function farthestFutureMs(values: Array<number | null>, nowMs: number): number | null {
  let out: number | null = null;
  for (const value of values) {
    if (value === null || value <= nowMs) {
      continue;
    }
    if (out === null || value > out) {
      out = value;
    }
  }
  return out;
}

function rangeFromResetOrRolling(resetMs: number | null, durationMs: number, nowMs: number): {
  fromUnixMs: number;
  toUnixMs: number;
} {
  if (resetMs !== null && resetMs > nowMs) {
    const fromUnixMs = Math.max(0, resetMs - durationMs);
    if (fromUnixMs <= nowMs) {
      return {
        fromUnixMs,
        toUnixMs: nowMs
      };
    }
  }
  return {
    fromUnixMs: Math.max(0, nowMs - durationMs),
    toUnixMs: nowMs
  };
}

function rangeForFiveHourWithResetFallback(resetMs: number | null, nowMs: number): {
  fromUnixMs: number;
  toUnixMs: number;
} {
  const fiveHoursMs = 5 * HOUR_MS;
  const rollingFrom = Math.max(0, nowMs - fiveHoursMs);
  if (resetMs === null) {
    return {
      fromUnixMs: rollingFrom,
      toUnixMs: nowMs
    };
  }

  // Dynamic 5h window:
  // use the nearest past cooldown boundary among (reset-1d, reset-1w),
  // but never look back more than 5h.
  const candidates = [resetMs - DAY_MS, resetMs - WEEK_MS].filter((value) => value <= nowMs);
  const nearestPast = candidates.length > 0 ? Math.max(...candidates) : null;
  const baseFrom = nearestPast === null ? rollingFrom : Math.max(rollingFrom, nearestPast);
  return {
    fromUnixMs: Math.max(0, baseFrom),
    toUnixMs: nowMs
  };
}

function buildCodexWindowSpec(
  window: "primary" | "secondary" | "code_review",
  source: Record<string, unknown>,
  nowMs: number
): UsageWindowSpec | null {
  const durationSeconds = asNumber(source.limit_window_seconds ?? source.limitWindowSeconds);
  if (durationSeconds === null || durationSeconds <= 0) {
    return null;
  }
  const resetMs = parseResetMs(toResetAt(source.reset_at ?? source.resetAt));
  const range = rangeFromResetOrRolling(
    resetMs,
    Math.max(1, Math.round(durationSeconds * 1000)),
    nowMs
  );
  return {
    window,
    fromUnixMs: range.fromUnixMs,
    toUnixMs: range.toUnixMs
  };
}

function buildCodexWindowSpecsFromPayload(
  payload: unknown,
  nowMs: number
): UsageWindowSpec[] {
  const root = asRecord(payload);
  if (!root) {
    return [];
  }

  const out: UsageWindowSpec[] = [];
  const rateLimit = asRecord(root.rate_limit);
  if (rateLimit) {
    const primary = asRecord(rateLimit.primary_window);
    const secondary = asRecord(rateLimit.secondary_window);
    if (primary) {
      const spec = buildCodexWindowSpec("primary", primary, nowMs);
      if (spec) {
        out.push(spec);
      }
    }
    if (secondary) {
      const spec = buildCodexWindowSpec("secondary", secondary, nowMs);
      if (spec) {
        out.push(spec);
      }
    }
  }

  const codeReview = asRecord(root.code_review_rate_limit);
  if (codeReview) {
    const codeReviewWindow =
      asRecord(codeReview.primary_window) ?? asRecord(codeReview.secondary_window);
    if (codeReviewWindow) {
      const spec = buildCodexWindowSpec("code_review", codeReviewWindow, nowMs);
      if (spec) {
        out.push(spec);
      }
    }
  }

  return out;
}

function readResetValuesForChannel(
  channel: string,
  payload: unknown,
  liveRows: LiveUsageRow[],
  nowMs: number
): {
  nearestReset: number | null;
  farthestReset: number | null;
} {
  const values: Array<number | null> = liveRows.map((row) => parseResetMs(row.resetAt));
  const root = asRecord(payload);
  const normalized = channel.trim().toLowerCase();
  if (root && normalized === "antigravity") {
    const models = asRecord(root.models) ?? asRecord(root.model_usage);
    if (models) {
      for (const raw of Object.values(models)) {
        const model = asRecord(raw);
        const quota = asRecord(model?.quotaInfo) ?? asRecord(model?.quota_info) ?? model;
        values.push(
          parseResetMs(
            (quota?.resetTime as string | number | null | undefined) ??
              (quota?.reset_time as string | number | null | undefined) ??
              null
          )
        );
      }
    }
  }
  if (root && normalized === "geminicli") {
    const buckets = Array.isArray(root.buckets) ? root.buckets : [];
    for (const item of buckets) {
      const bucket = asRecord(item);
      values.push(
        parseResetMs((bucket?.resetTime as string | number | null | undefined) ?? null)
      );
    }
  }

  return {
    nearestReset: nearestFutureMs(values, nowMs),
    farthestReset: farthestFutureMs(values, nowMs)
  };
}

export function buildUsageWindowSpecs(
  channel: string,
  payload: unknown,
  liveRows: LiveUsageRow[],
  nowMs: number
): UsageWindowSpec[] {
  const normalized = channel.trim().toLowerCase();
  if (normalized === "codex") {
    const codexWindows = buildCodexWindowSpecsFromPayload(payload, nowMs);
    return codexWindows;
  }
  const { nearestReset, farthestReset } = readResetValuesForChannel(
    channel,
    payload,
    liveRows,
    nowMs
  );
  const windows: UsageWindowSpec[] = [];
  const add = (window: UsageWindow, fromUnixMs: number, toUnixMs: number) => {
    windows.push({ window, fromUnixMs, toUnixMs });
  };

  if (normalized === "geminicli" || normalized === "antigravity") {
    const range5h = rangeForFiveHourWithResetFallback(nearestReset, nowMs);
    const range1d = rangeFromResetOrRolling(nearestReset, DAY_MS, nowMs);
    const range1w = rangeFromResetOrRolling(farthestReset ?? nearestReset, WEEK_MS, nowMs);
    add("5h", range5h.fromUnixMs, range5h.toUnixMs);
    add("1d", range1d.fromUnixMs, range1d.toUnixMs);
    add("1w", range1w.fromUnixMs, range1w.toUnixMs);
    add("sum", 0, nowMs);
    return windows;
  }

  if (normalized === "claudecode") {
    const range5h = rangeFromResetOrRolling(nearestReset, 5 * HOUR_MS, nowMs);
    const range1d = rangeFromResetOrRolling(nearestReset, DAY_MS, nowMs);
    const range1w = rangeFromResetOrRolling(farthestReset ?? nearestReset, WEEK_MS, nowMs);
    add("5h", range5h.fromUnixMs, range5h.toUnixMs);
    add("1d", range1d.fromUnixMs, range1d.toUnixMs);
    add("1w", range1w.fromUnixMs, range1w.toUnixMs);
    add("sum", 0, nowMs);
    return windows;
  }

  add("1w", Math.max(0, nowMs - WEEK_MS), nowMs);
  add("sum", 0, nowMs);
  return windows;
}

export function formatUsagePercent(value: number | null): string {
  if (value === null || !Number.isFinite(value)) {
    return "-";
  }
  const rounded = Math.round(value * 10) / 10;
  return `${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)}%`;
}

function stripGeminiModelLabel(name: string): string {
  const trimmed = normalizeModelLabel(name);
  if (!trimmed) {
    return trimmed;
  }
  const withType = trimmed.match(/^(.*)\s+\([A-Za-z_]+\)$/);
  if (withType && withType[1]) {
    return withType[1].trim();
  }
  return trimmed;
}

function uniqueStrings(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const value of values) {
    const key = value.trim();
    if (!key || seen.has(key)) {
      continue;
    }
    seen.add(key);
    out.push(key);
  }
  return out;
}

function parseAtUnixMs(value: unknown): number | null {
  return parseAtToUnixMs(value);
}

function rowInWindow(atUnixMs: number | null, spec: UsageWindowSpec): boolean {
  if (atUnixMs === null) {
    return false;
  }
  return atUnixMs >= spec.fromUnixMs && atUnixMs < spec.toUnixMs;
}

function asToken(value: number | null): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function summarizeRows(
  rows: UsageSampleRow[],
  spec: UsageWindowSpec,
  matcher: (row: UsageSampleRow) => boolean
): UsageDisplayRow {
  let calls = 0;
  let inputTokens = 0;
  let outputTokens = 0;
  let cacheReadTokens = 0;
  let cacheCreationTokens = 0;
  let cacheCreationTokens5m = 0;
  let cacheCreationTokens1h = 0;
  let cacheTokens = 0;
  let totalTokens = 0;

  for (const row of rows) {
    if (!matcher(row)) {
      continue;
    }
    const atUnixMs = parseAtUnixMs(row.at);
    if (!rowInWindow(atUnixMs, spec)) {
      continue;
    }
    const input = asToken(row.input_tokens);
    const output = asToken(row.output_tokens);
    const cacheRead = asToken(row.cache_read_input_tokens);
    const cacheCreation = asToken(row.cache_creation_input_tokens);
    const cacheCreation5m = asToken(row.cache_creation_input_tokens_5min);
    const cacheCreation1h = asToken(row.cache_creation_input_tokens_1h);
    const cache = cacheRead + cacheCreation;
    calls += 1;
    inputTokens += input;
    outputTokens += output;
    cacheReadTokens += cacheRead;
    cacheCreationTokens += cacheCreation;
    cacheCreationTokens5m += cacheCreation5m;
    cacheCreationTokens1h += cacheCreation1h;
    cacheTokens += cache;
    totalTokens += input + output + cache;
  }

  return {
    label: "",
    window: spec.window,
    fromUnixMs: spec.fromUnixMs,
    toUnixMs: spec.toUnixMs,
    calls,
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheCreationTokens,
    cacheCreationTokens5m,
    cacheCreationTokens1h,
    cacheTokens,
    totalTokens
  };
}

function buildTokenRowsByLabel(
  labels: string[],
  specs: UsageWindowSpec[],
  rows: UsageSampleRow[],
  labelMatcher: (label: string, row: UsageSampleRow) => boolean
): UsageDisplayRow[] {
  const out: UsageDisplayRow[] = [];
  for (const label of labels) {
    for (const spec of specs) {
      const summary = summarizeRows(rows, spec, (row) => labelMatcher(label, row));
      out.push({ ...summary, label });
    }
  }
  return out;
}

export function buildUsageDisplayRows(
  channel: string,
  liveRows: LiveUsageRow[],
  specs: UsageWindowSpec[],
  usageRows: UsageSampleRow[]
): { kind: UsageDisplayKind; rows: UsageDisplayRow[] } {
  const normalized = channel.trim().toLowerCase();

  if (normalized === "codex") {
    const modelLabels = uniqueStrings(
      usageRows
        .map((row) => normalizeModelLabel(row.model ?? ""))
        .filter(Boolean)
    ).sort((a, b) => a.localeCompare(b));
    return {
      kind: "tokens",
      rows: buildTokenRowsByLabel(
        modelLabels.length > 0 ? modelLabels : ["all"],
        specs,
        usageRows,
        (model, row) =>
          model === "all" ? true : normalizeModelLabel(row.model ?? "") === model
      )
    };
  }

  if (normalized === "claudecode") {
    const groups = ["haiku", "sonnet", "opus"];
    return {
      kind: "tokens",
      rows: buildTokenRowsByLabel(groups, specs, usageRows, (group, row) => {
        const model = row.model?.toLowerCase() ?? "";
        return model.includes(group);
      })
    };
  }

  if (normalized === "antigravity") {
    const fromLive = liveRows.map((row) => normalizeModelLabel(row.name)).filter(Boolean);
    const fromUsage = usageRows
      .map((row) => normalizeModelLabel(row.model ?? ""))
      .filter(Boolean);
    const labels = uniqueStrings([...fromLive, ...fromUsage]).sort((a, b) =>
      a.localeCompare(b)
    );
    return {
      kind: "tokens",
      rows: buildTokenRowsByLabel(
        labels.length > 0 ? labels : ["all"],
        specs,
        usageRows,
        (model, row) =>
          model === "all" ? true : normalizeModelLabel(row.model ?? "") === model
      )
    };
  }

  if (normalized === "geminicli") {
    const fromLive = liveRows.map((row) => stripGeminiModelLabel(row.name));
    const fromUsage = usageRows
      .map((row) => normalizeModelLabel(row.model ?? ""))
      .filter(Boolean);
    const modelLabels = uniqueStrings([...fromLive, ...fromUsage]).sort((a, b) =>
      a.localeCompare(b)
    );
    const rows = buildTokenRowsByLabel(modelLabels, specs, usageRows, (model, row) => {
      return normalizeModelLabel(row.model ?? "") === model;
    });
    return {
      kind: "tokens",
      rows
    };
  }

  return {
    kind: "calls",
    rows: buildTokenRowsByLabel(["all"], specs, usageRows, () => true)
  };
}

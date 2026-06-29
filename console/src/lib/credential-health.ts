export interface CredentialHealthLike {
  health_kind: string;
  health_json: { open_until?: number } | null;
}

export interface DatedCredentialHealthLike extends CredentialHealthLike {
  updated_at: number;
}

function isExpiredRateLimit(status: CredentialHealthLike, nowSecs: number): boolean {
  const until = status.health_json?.open_until;
  return (
    status.health_kind === "rate_limited" &&
    typeof until === "number" &&
    Number.isFinite(until) &&
    until <= nowSecs
  );
}

export function isCurrentCredentialStatus(status: CredentialHealthLike, nowSecs: number): boolean {
  return !isExpiredRateLimit(status, nowSecs);
}

export function currentCredentialStatuses<T extends CredentialHealthLike>(
  rows: T[],
  nowSecs: number,
): T[] {
  return rows.filter((row) => isCurrentCredentialStatus(row, nowSecs));
}

export function latestCurrentCredentialStatus<T extends DatedCredentialHealthLike>(
  rows: T[],
  nowSecs: number,
): T | undefined {
  return [...currentCredentialStatuses(rows, nowSecs)].sort((a, b) => b.updated_at - a.updated_at)[0];
}

export function unixNow(): number {
  return Math.floor(Date.now() / 1000);
}

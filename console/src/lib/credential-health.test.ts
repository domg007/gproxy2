import { describe, expect, it } from "vitest";
import {
  currentCredentialStatuses,
  latestCurrentCredentialStatus,
  type DatedCredentialHealthLike,
} from "./credential-health";

describe("credential health status helpers", () => {
  it("ignores expired rate-limit cooldowns", () => {
    const rows: DatedCredentialHealthLike[] = [
      {
        health_kind: "rate_limited",
        health_json: { open_until: 100 },
        updated_at: 20,
      },
      {
        health_kind: "recovered",
        health_json: null,
        updated_at: 10,
      },
    ];

    expect(currentCredentialStatuses(rows, 101).map((row) => row.health_kind)).toEqual([
      "recovered",
    ]);
    expect(latestCurrentCredentialStatus(rows, 101)?.health_kind).toBe("recovered");
  });
});

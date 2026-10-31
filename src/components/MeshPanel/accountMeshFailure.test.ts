import { describe, expect, it } from "vitest";
import { failureMessage } from "./accountMachines";
import { type AccountMeshFailure } from "./accountMeshClient";

const REASONS = ["revoked", "cancelled", "verification-unavailable", "key_conflict", "generation_mismatch", "challenge_invalid", "enrollment_budget", "proof_invalid"] as const;

describe("account mesh failure reasons", () => {
  it("explains rejected proofs with QA guidance only for DEV builds", () => {
    const failure = { reason: "proof_invalid" } as const;
    expect(failureMessage(failure, false)).toBe("This build cannot join this account's mesh (proof rejected).");
    expect(failureMessage(failure, true)).toBe(`${failureMessage(failure, false)} DEV builds use the QA target.`);
  });
  it.each(REASONS)("gives %s its own message", (reason) => {
    const message = failureMessage({ reason } as AccountMeshFailure);
    expect(message).toBeTruthy();
    expect(message).not.toBe(failureMessage({ reason: "unavailable" }));
  });

  it("preserves the server deadline in rate-limit copy", () => {
    expect(failureMessage({ reason: "rate-limited", retry_after_seconds: 60 }, false, 1000)).toContain(new Date(1000000).toISOString());
  });
});

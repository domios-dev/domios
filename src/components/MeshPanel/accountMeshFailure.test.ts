import { describe, expect, it } from "vitest";
import { AccountMeshRefusalError } from "../billing/accountMeshLimit";
import { failureMessage } from "./accountMachines";
import { type AccountMeshFailure, linkFailure } from "./accountMeshClient";

const REASONS = ["revoked", "cancelled", "verification-unavailable"] as const;

describe("account mesh failure reasons", () => {
  it.each(REASONS)("keeps a %s refusal typed instead of collapsing it to unavailable", (reason) => {
    const error = new AccountMeshRefusalError({ requestId: "req", refusal: { reason } });
    expect(linkFailure(error)).toEqual({ reason });
  });

  it.each(REASONS)("gives %s its own message", (reason) => {
    const message = failureMessage({ reason } as AccountMeshFailure);
    expect(message).toBeTruthy();
    expect(message).not.toBe(failureMessage({ reason: "unavailable" }));
  });

  it("preserves the mesh rate-limit retry delay in the UI message", () => {
    const error = new AccountMeshRefusalError({
      requestId: "req",
      refusal: { reason: "rate-limited", retry_after_seconds: 60 },
    });
    const failure = linkFailure(error);

    expect(failure).toEqual({ reason: "rate-limited", retry_after_seconds: 60 });
    expect(failureMessage(failure)).toContain("60 seconds");
    expect(failureMessage(failure)).not.toContain("undefined");
  });
});

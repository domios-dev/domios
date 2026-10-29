import { describe, expect, it } from "vitest";
import { enrollmentRetryMessage, myMachinesView } from "./accountMachines";
import { macRow, NOW, snapshot } from "./accountMachines.testdata";

describe("enrollment budget projection", () => {
  it("keeps cached machines readable and all registry controls read-only", () => {
    const state = snapshot({ enrollment: "refused", last_refusal: { reason: "enrollment_budget", retry_after_seconds: 121 }, retry_at: NOW + 121 });
    const view = myMachinesView({ account: "signed_in", snapshot: state, linking: new Set(), now: NOW });
    if (view.kind !== "account") throw new Error("Account machines missing");
    const row = view.machines.find(row => row.key === macRow.installation_id);
    expect(row?.name).toBe("MacBook");
    expect(row?.controls).toEqual({ add: null, revoke: null, unpairInstanceId: null });
    expect(row?.canSignOut).toBe(false);
    expect(view.self?.readOnly).toBe(true);
    expect(enrollmentRetryMessage(state, NOW)).toContain("Try again in 3 min");
    expect(view.notes).toContain(enrollmentRetryMessage(state, NOW));
    expect(enrollmentRetryMessage(state, NOW + 61)).toContain("Try again in 1 min");
  });

  it("uses no countdown without a server deadline and stops promising an exhausted retry", () => {
    const state = snapshot({ enrollment: "refused", last_refusal: { reason: "enrollment_budget" }, retry_at: NOW + 900 });
    expect(enrollmentRetryMessage(state, NOW)).toBe("Account enrollment limit reached, retrying automatically");
    expect(enrollmentRetryMessage({ ...state, retry_at: null }, NOW + 900)).toBe("Account enrollment limit reached. Try again later.");
    expect(enrollmentRetryMessage({ ...state, enrollment: "enrolled" }, NOW)).toBeNull();
  });
});

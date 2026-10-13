import { describe, expect, it } from "vitest";
import { modelSwitchFailure } from "./modelSwitchFailure";
import { TransportError } from "./transportClient";

function refusal(reason: string, fallback?: boolean) {
  return new TransportError(409, "/api/tabs/owned/agent/model", JSON.stringify({
    ok: false, reason, error: "Provider refused: settings unavailable", restart_fallback_available: fallback,
  }));
}

describe("model switch failure", () => {
  it("shows the full typed refusal rather than the request path and never offers restart", () => {
    const error = refusal("runtime_control_refused");
    expect(modelSwitchFailure(error, true)).toEqual({ message: "Provider refused: settings unavailable", restartAvailable: false });
    expect(modelSwitchFailure(error, false).restartAvailable).toBe(false);
  });

  it("suppresses fallback for runtime backing even if another failure advertises it", () => {
    expect(modelSwitchFailure(refusal("live_command_failed", true), true).restartAvailable).toBe(false);
    expect(modelSwitchFailure(new Error("opaque refusal"), true).restartAvailable).toBe(false);
  });

  it("keeps explicitly offered PTY restart and preserves ordinary errors", () => {
    expect(modelSwitchFailure(refusal("live_command_failed", true), false).restartAvailable).toBe(true);
    expect(modelSwitchFailure(refusal("invalid_model"), false).restartAvailable).toBe(false);
    expect(modelSwitchFailure(new Error("opaque failure"), false)).toEqual({ message: "opaque failure", restartAvailable: true });
  });
});

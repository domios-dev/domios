import { describe, expect, it, vi } from "vitest";
vi.mock("./bridge", () => ({ invoke: vi.fn(async () => undefined) }));
import { invoke } from "./bridge";
import { makeOrchestrator, makePlanner } from "./orchestratorOnboard";

describe("orchestration role caller claims", () => {
  it("uses each target tab's exact nonce", async () => {
    await makeOrchestrator("first", "first-nonce");
    expect(invoke).toHaveBeenLastCalledWith("mux_make_orchestrator", { tabId: "first", callerTabId: "first", sessionBindingNonce: "first-nonce" });
    await makePlanner("second", "plan", "second-nonce");
    expect(invoke).toHaveBeenLastCalledWith("mux_make_planner", { tabId: "second", planId: "plan", callerTabId: "second", sessionBindingNonce: "second-nonce" });
  });
  it("refuses a missing binding before dispatch", async () => {
    vi.mocked(invoke).mockClear();
    await expect(makeOrchestrator("first")).rejects.toThrow("current session binding nonce");
    await expect(makePlanner("second", "plan")).rejects.toThrow("current session binding nonce");
    expect(invoke).not.toHaveBeenCalled();
  });
});

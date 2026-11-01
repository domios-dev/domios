import { describe, expect, it } from "vitest";
import { updatePresentation, updateStatusForPhase } from "./updatePresentation";
import { updateStageView } from "./updateStageView";

describe("app update presentation", () => {
  it.each(["checking", "downloading"] as const)("keeps %s usable during a user install", status => {
    expect(updatePresentation(status, true, "1.17.2")).toMatchObject({
      blocking: false, background: true, retry: false,
    });
  });

  it("keeps verification and preflight non-blocking while the shell retains downloading", () => {
    const status = updateStatusForPhase("downloading", true);
    expect(status).toBe("downloading");
    expect(updatePresentation(status!, true, "1.17.2").blocking).toBe(false);
  });

  it.each(["applying", "restarting"] as const)("blocks only the unusable %s stage", status => {
    expect(updatePresentation(status, true, "1.17.2")).toEqual({
      blocking: true, background: false, retry: false, title: "Restarting to install 1.17.2…",
    });
    expect(updateStageView(status, "1.17.2").label).toBe("Restarting to install 1.17.2…");
  });

  it("maps shutdown/install and restart shell phases to blocking presentation", () => {
    expect(updateStatusForPhase("applying", true)).toBe("applying");
    expect(updateStatusForPhase("success", true)).toBe("restarting");
  });

  it("replaces background progress with a non-blocking error and Retry", () => {
    expect(updateStatusForPhase("error", true)).toBe("error");
    expect(updatePresentation("error", true, "1.17.2")).toEqual({
      blocking: false, background: true, retry: true, title: "Update failed",
    });
    expect(updatePresentation("error", false, "1.17.2").blocking).toBe(false);
  });

  it.each(["idle", "available", "up-to-date"] as const)("does not show install progress for %s", status => {
    expect(updatePresentation(status, false, null)).toMatchObject({ blocking: false, background: false });
  });
});

import { describe, expect, finish, it } from "../../test/standaloneTest";
import { inputWriteFailureTarget, reduceInputWriteFailure } from "./inputWriteFailureModel";

describe("input failure model", () => {
  it("sets only the current target and clears on its success or dismissal", () => {
    const target = inputWriteFailureTarget("tab", "pty")!;
    expect(reduceInputWriteFailure(null, target, { kind: "failure", targetId: "other" })).toBe(null);
    const failed = reduceInputWriteFailure(null, target, { kind: "failure", targetId: target });
    expect(failed).toBe(target);
    expect(reduceInputWriteFailure(failed, target, { kind: "success", targetId: "other" })).toBe(target);
    expect(reduceInputWriteFailure(failed, target, { kind: "success", targetId: target })).toBe(null);
    expect(reduceInputWriteFailure(failed, target, { kind: "dismiss" })).toBe(null);
  });
  it("isolates composite targets and discards stale PTY identity", () => {
    const first = inputWriteFailureTarget("mesh:a/tab", "shared")!;
    const second = inputWriteFailureTarget("mesh:b/tab", "shared")!;
    expect(first).toBe("mesh:a/tab");
    expect(second).toBe("mesh:b/tab");
    expect(inputWriteFailureTarget("local", "shared")).toBe("shared");
    expect(inputWriteFailureTarget("local", null)).toBe(null);
    expect(reduceInputWriteFailure(first, second, { kind: "success", targetId: first })).toBe(null);
    expect(reduceInputWriteFailure("old", "new", { kind: "failure", targetId: "old" })).toBe(null);
  });
});
finish();

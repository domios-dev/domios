import { describe, expect, finish, it } from "../../test/standaloneTest";
import { replayOffsetsAfterBootChange } from "../../lib/outputStream";
import { terminalHistorySource } from "./scrollbackReplay";

describe("restored terminal history ownership", () => {
  it("reads restored native history from the seeded backend buffer", () => {
    expect(terminalHistorySource("pty", false, false)).toBe("backend-buffer");
  });

  it("uses one web transport owner for history and live output", () => {
    expect(terminalHistorySource("pty", true, false)).toBe("transport");
  });

  it("leaves remote history to its cursor-aware stream", () => {
    expect(terminalHistorySource("remote-pty", false, true)).toBe("transport");
  });

  it("does not replay history before a backend pty exists", () => {
    expect(terminalHistorySource(null, false, false)).toBe("none");
  });
});

describe("web output stream restart replay", () => {
  it("restarts each subscribed tab at offset zero for the new backend buffer", () => {
    expect([...replayOffsetsAfterBootChange(["tab-a", "tab-b"])]).toEqual([
      ["tab-a", 0],
      ["tab-b", 0],
    ]);
  });
});

finish();

import { describe, expect, finish, it } from "../../test/standaloneTest";
import { shouldFetchInitialTerminalHistory } from "./usePtyConnection";
import { terminalHistorySource } from "./scrollbackReplay";

describe("remote terminal initial history ownership", () => {
  it("leaves remote replay to the cursor-aware output stream", () => {
    expect(shouldFetchInitialTerminalHistory("mesh:peer/tab", false, true)).toBe(false);
  });

  it("keeps native local history replay and web stream ownership", () => {
    expect(shouldFetchInitialTerminalHistory("pty", false, false)).toBe(true);
    expect(shouldFetchInitialTerminalHistory("pty", true, false)).toBe(false);
    expect(shouldFetchInitialTerminalHistory(null, false, false)).toBe(false);
  });

  it("gives restored history exactly one initial replay owner per transport", () => {
    expect(terminalHistorySource("pty", false, false)).toBe("backend-buffer");
    expect(terminalHistorySource("pty", true, false)).toBe("transport");
  });
});

finish();

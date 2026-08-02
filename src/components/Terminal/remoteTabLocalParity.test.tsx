import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { tabContentSurface } from "./TabContent";

describe("remote tab local component parity", () => {
  it("uses TerminalTab, TabSessionChat and TabTopBar without a parallel remote surface", () => {
    const source = readFileSync(new URL("./TabContent.tsx", import.meta.url), "utf8");
    expect(source).toContain("<TerminalTab");
    expect(source).toContain("<TabSessionChat");
    expect(source).toContain("<TabTopBar");
    expect(source).not.toContain("RemoteTabSurface");
    expect(source).toContain('tab.remote_availability !== "available"');
    const local: Tab = { id: "same", title: "Local", pty_id: "same", tab_type: "Terminal", cwd: null };
    const remote: Tab = {
      ...local,
      id: "mesh:peer/same",
      pty_id: "mesh:peer/same",
      remote_peer_id: "peer",
      remote_tab_id: "same",
      remote_availability: "available",
    };
    expect(tabContentSurface(local)).toBe("ordinary");
    expect(tabContentSurface(remote)).toBe("ordinary");
  });
});

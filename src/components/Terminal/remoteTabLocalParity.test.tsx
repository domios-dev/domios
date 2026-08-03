import { readFileSync } from "node:fs";
import { describe, expect, finish, it } from "../../test/standaloneTest";

describe("remote tab local component parity", () => {
  it("uses TerminalTab, TabSessionChat and TabTopBar without a parallel remote surface", () => {
    const source = readFileSync(new URL("./TabContent.tsx", import.meta.url), "utf8");
    expect(source).toContain("<TerminalTab");
    expect(source).toContain("<TabSessionChat");
    expect(source).toContain("<TabTopBar");
    expect(source).not.toContain("RemoteTabSurface");
    expect(source).not.toContain("RemoteTabUnavailable");
    expect(source).not.toContain("remote_availability");
    const storeSource = readFileSync(new URL("../../store/multiplexer/index.tsx", import.meta.url), "utf8");
    expect(storeSource).not.toContain('remote.remote_availability !== "available"');
    const routeSource = readFileSync(new URL("../../lib/remoteTabRoute.ts", import.meta.url), "utf8");
    expect(routeSource).not.toContain("remote_availability");
  });
});

finish();

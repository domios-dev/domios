import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const tabsTransport = readFileSync(resolve(process.cwd(), "src/lib/transport/tabs.ts"), "utf8");
const orchestrationTransport = readFileSync(resolve(process.cwd(), "src/lib/transport/orchestration.ts"), "utf8");
const sidebar = readFileSync(resolve(process.cwd(), "src/components/Sidebar/Sidebar.tsx"), "utf8");
const multiplexer = readFileSync(resolve(process.cwd(), "src/store/multiplexer/index.tsx"), "utf8");

describe("confirmed close UI contracts", () => {
  it("posts force and the exact target confirmation for workspace and Team closes", () => {
    expect(tabsTransport).toMatch(/mux_close_workspace:[\s\S]*?force: a\.force === true,[\s\S]*?confirm: s\(a, "workspaceId"\)/);
    expect(orchestrationTransport).toMatch(
      /mux_close_team:[\s\S]*?\{ force: true, confirm: args\.teamId, force_busy: args\.forceBusy === true,/,
    );
  });

  it("awaits Team close and turns rejection into a failure toast", () => {
    expect(sidebar).toMatch(/await closeTeam\(pending\.teamId\)/);
    expect(sidebar).toMatch(/catch \(error\) \{[\s\S]*?title: "Team close failed"[\s\S]*?level: "failure"/);
  });

  it("turns a rejected workspace close into a failure toast", () => {
    expect(multiplexer).toMatch(/\.catch\(\(error\) => \{[\s\S]*?title: "Workspace close failed"[\s\S]*?level: "failure"/);
  });
});

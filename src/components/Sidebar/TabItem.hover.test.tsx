import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const browser: Tab = {
  id: "browser", title: "Browser", tab_type: "Terminal", pty_id: null, cwd: null,
  parent_tab_id: "main",
  backing: { kind: "surface", plugin_id: "browser", surface_id: "browser", instance_id: "fixture" },
};
const probe: Tab = {
  id: "probe", title: "Probe", tab_type: "Terminal", pty_id: null, cwd: null,
  agent_type: "fixture-agent", description: "Fixture task",
};

describe("sidebar row hover wiring", () => {
  it("keeps one named activation and action target in each adjacent row", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const openBrowser = vi.fn();
    const openProbe = vi.fn();
    try {
      await act(async () => root.render(<>
        <TabItem tab={browser} index={0} variant="attachment" isActive={false}
          onClick={openBrowser} onClose={() => undefined} canClose workspaceId="fixture" />
        <TabItem tab={probe} index={1} isActive={false}
          onClick={openProbe} onClose={() => undefined} canClose workspaceId="fixture" />
      </>));
      const rows = host.querySelectorAll('[role="group"]');
      expect(rows).toHaveLength(2);
      for (const [index, name] of ["Browser", "Probe"].entries()) {
        const row = rows[index];
        expect(row.getAttribute("aria-label")).toBe(name);
        expect(row.querySelectorAll('[role="group"]')).toHaveLength(0);
        const activation = row.querySelectorAll<HTMLButtonElement>(`button[aria-label="Open tab ${name}"]`);
        expect(activation).toHaveLength(1);
        expect(row.querySelectorAll('button[aria-label="Tab actions"][aria-haspopup="menu"]')).toHaveLength(1);
        expect(row.querySelectorAll('[title="' + name + '"]')).toHaveLength(0);
        await act(async () => activation[0].click());
      }
      expect(openBrowser).toHaveBeenCalledTimes(1);
      expect(openProbe).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { IS_WEB, invoke } from "../../lib/bridge";
import type { Workspace } from "../../lib/types";
import { WorkspaceItem } from "./WorkspaceItem";

vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/bridge")>()),
  invoke: vi.fn(async () => []),
  listen: vi.fn(async () => () => undefined),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => document.body.replaceChildren());

const workspace: Workspace = { id: "workspace", name: "Workspace", tabs: [], active_tab_id: null, cwd: "/workspace" };

it("marks Show in Explorer unavailable on the web instead of failing silently", async () => {
  expect(IS_WEB).toBe(true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const noop = () => undefined;
  try {
    await act(async () => root.render(
      <WorkspaceItem workspace={workspace} isActive isExpanded={false} onClick={noop} onToggleExpand={noop}
        onClose={noop} onRename={noop} onNewTerminal={noop} onNewSession={noop} onNewAgent={noop} canClose status={null} />,
    ));
    const trigger = host.querySelector('button[aria-label="Actions for Workspace"]')!;
    await act(async () => { trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })); });
    const reveal = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menu"] [role="menuitem"]'))
      .find((item) => /Show in (Explorer|Finder)/.test(item.textContent ?? ""));
    expect(reveal).toBeDefined();
    expect(reveal!.getAttribute("aria-disabled")).toBe("true");
    const describedBy = reveal!.getAttribute("aria-describedby");
    expect(describedBy && document.getElementById(describedBy)?.textContent).toContain("desktop app");
    vi.mocked(invoke).mockClear();
    await act(async () => reveal!.click());
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("open_folder", expect.anything());
  } finally {
    await act(async () => root.unmount());
  }
});

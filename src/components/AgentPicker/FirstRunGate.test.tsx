import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { firstRunLocked, firstRunSelection, firstRunSidebarTabs, FIRST_RUN_DISABLED_REASON } from "../../lib/firstRunGate";
import { useAppShortcuts } from "../../hooks/useAppShortcuts";
import { APP_SHORTCUT_ACTIONS, type AppShortcutHandlers } from "../../lib/appShortcuts";
import type { Tab, Workspace } from "../../lib/types";
import { isGeneralSectionTab } from "../Sidebar/workspaceTabs";
import { CommandPalette } from "../CommandPalette/CommandPalette";
import { FirstRunControls } from "./FirstRunControls";
import { FIRST_RUN_GA_TAB_MARKER } from "./constants";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const home: Tab = { id: "home", title: "Get started", description: FIRST_RUN_GA_TAB_MARKER, tab_type: "Terminal", pty_id: null, cwd: null };
const api: Tab = { id: "api", title: "Linkfixture", agent_type: "fixture", tab_type: "Terminal", pty_id: null, cwd: null };
const workspace: Workspace = { id: "general", name: "General", cwd: null, protected: true, tabs: [api.id, home.id], active_tab_id: api.id };
let dispose: (() => Promise<void>) | undefined;
afterEach(async () => { await dispose?.(); dispose = undefined; });
async function mount(element: React.ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  dispose = async () => { await act(async () => root.unmount()); host.remove(); };
  await act(async () => root.render(element));
  return { host, root };
}

it("uses the persisted completion flag and fails closed until it is loaded", () => {
  expect(firstRunLocked(undefined)).toBe(true);
  expect(firstRunLocked({})).toBe(true);
  expect(firstRunLocked({ first_run_complete: false })).toBe(true);
  expect(firstRunLocked({ first_run_complete: true })).toBe(false);
});

it("pins selection and hides API tabs without changing their ids or workspace membership", () => {
  const tabs = { home, api };
  const state = { workspaces: [workspace], tabs, active_workspace_id: workspace.id,
    activeTabIds: { general: api.id }, focused_tab_id: api.id };
  const pinned = firstRunSelection(true, state);
  expect(pinned.focused_tab_id).toBe(home.id);
  expect(pinned.activeTabIds.general).toBe(home.id);
  expect(pinned.tabs).toBe(tabs);
  expect(pinned.workspaces).toBe(state.workspaces);
  expect(Object.keys(firstRunSidebarTabs(true, tabs))).toEqual([home.id]);
  expect(firstRunSidebarTabs(false, tabs)).toBe(tabs);
  expect(firstRunSelection(false, state)).toBe(state);
  expect(workspace.tabs).toEqual([api.id, home.id]);
  expect(firstRunSelection(true, { ...state, tabs: { api } }).focused_tab_id).toBeNull();
});

it("places only the first-run tab and actual General role in the General card", () => {
  expect(isGeneralSectionTab(home)).toBe(true);
  expect(isGeneralSectionTab(api)).toBe(false);
  expect(isGeneralSectionTab({ ...api, protected: true })).toBe(false);
  expect(isGeneralSectionTab({ ...api, role: "Manager", role_profile: "orchestrator" })).toBe(false);
  expect(isGeneralSectionTab({ ...api, role: "Manager", role_profile: "general_agent" })).toBe(true);
});

it("keeps shell controls mounted and disables navigation, creation and menus in place", async () => {
  const navigate = vi.fn();
  const render = (locked: boolean, appended = false) => <FirstRunControls locked={locked}>
    <section aria-label="General"><button data-first-run-allowed="true" data-first-run-home="true">Get started</button></section>
    <section aria-label="Workspaces"><button onClick={navigate}>New workspace</button><button>New session</button></section>
    <nav><button onClick={navigate}>Mesh</button><button>Memory</button><button>Plugins</button></nav>
    <button disabled>Unavailable</button><button onClick={navigate}>Settings</button>
    <a href="#other" onClick={navigate}>Other navigation</a>
    <span data-first-run-allowed="true"><button onClick={navigate}>Sign in</button></span>
    {appended && <button onClick={navigate}>Late control</button>}
  </FirstRunControls>;
  const { host, root } = await mount(render(true));
  const buttons = () => Array.from(host.querySelectorAll("button"));
  const workspaceButton = buttons().find((button) => button.textContent === "New workspace")!;
  expect(document.activeElement?.textContent).toBe("Get started");
  for (const button of buttons().filter((button) => !["Get started", "Sign in"].includes(button.textContent!))) {
    expect(button.disabled).toBe(true);
    expect(button.title).toBe(FIRST_RUN_DISABLED_REASON);
  }
  await act(async () => { workspaceButton.click(); host.querySelector("a")!.click(); });
  expect(navigate).not.toHaveBeenCalled();
  await act(async () => buttons().find((button) => button.textContent === "Sign in")!.click());
  expect(navigate).toHaveBeenCalledOnce();
  await act(async () => root.render(render(true, true)));
  expect(buttons().find((button) => button.textContent === "Late control")!.disabled).toBe(true);
  await act(async () => root.render(render(false, true)));
  expect(buttons().find((button) => button.textContent === "New workspace")).toBe(workspaceButton);
  expect(workspaceButton.disabled).toBe(false);
  expect(buttons().find((button) => button.textContent === "Unavailable")!.disabled).toBe(true);
  await act(async () => workspaceButton.click());
  expect(navigate).toHaveBeenCalledTimes(2);
});

it("keeps mouse and touch activation of Get started away from sortable drag handlers", async () => {
  const drag = vi.fn();
  const activate = vi.fn();
  const { host } = await mount(<div onMouseDown={drag} onTouchStart={drag}>
    <FirstRunControls locked><button data-first-run-allowed="true" data-first-run-home="true" onClick={activate}>Get started</button></FirstRunControls>
  </div>);
  const home = host.querySelector("button")!;
  await act(async () => {
    home.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    home.dispatchEvent(new Event("touchstart", { bubbles: true }));
    home.click();
  });
  expect(drag).not.toHaveBeenCalled();
  expect(activate).toHaveBeenCalledOnce();
});

it("lets the window drag from its title bar while controls in it stay locked", async () => {
  const windowDrag = vi.fn();
  const settings = vi.fn();
  const minimize = vi.fn();
  const { host } = await mount(<FirstRunControls locked>
    <div data-tauri-drag-region data-testid="bar">
      <span data-tauri-drag-region data-testid="strip" />
      <button onClick={settings}>Settings</button>
      <div data-first-run-allowed="true"><button onClick={minimize}>Minimize</button></div>
    </div>
  </FirstRunControls>);
  // The native drag listens for the press after the page; a press the gate swallowed never reaches it.
  document.addEventListener("mousedown", windowDrag);
  try {
    for (const id of ["bar", "strip"]) {
      const press = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
      await act(async () => host.querySelector(`[data-testid=${id}]`)!.dispatchEvent(press));
      expect(press.defaultPrevented).toBe(false);
    }
    expect(windowDrag).toHaveBeenCalledTimes(2);
    const buttons = Array.from(host.querySelectorAll("button"));
    await act(async () => buttons.forEach((button) => button.click()));
    expect(settings).not.toHaveBeenCalled();
    expect(minimize).toHaveBeenCalledOnce();
  } finally {
    document.removeEventListener("mousedown", windowDrag);
  }
});

it("blocks shortcuts and the palette until completion, then restores their handlers", async () => {
  const handlers = Object.fromEntries(APP_SHORTCUT_ACTIONS.map((action) => [action, vi.fn()])) as unknown as AppShortcutHandlers;
  function Shell({ locked }: { locked: boolean }) {
    useAppShortcuts({}, handlers, locked);
    return <CommandPalette disabled={locked} commands={[{ id: "new", label: "New session", action: vi.fn() }]} />;
  }
  const { root } = await mount(<Shell locked />);
  const press = (code: string, shiftKey = false) => window.dispatchEvent(new KeyboardEvent("keydown", { code, ctrlKey: true, shiftKey, bubbles: true }));
  await act(async () => { press("KeyT"); press("KeyN", true); press("KeyT", true); press("KeyW"); press("Tab"); press("Comma"); press("KeyK"); });
  for (const handler of Object.values(handlers)) expect(handler).not.toHaveBeenCalled();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  await act(async () => root.render(<Shell locked={false} />));
  await act(async () => { press("KeyT"); press("KeyN", true); press("Comma"); press("KeyK"); });
  expect(handlers.new_tab).toHaveBeenCalledOnce();
  expect(handlers.new_workspace).toHaveBeenCalledOnce();
  expect(handlers.open_settings).toHaveBeenCalledOnce();
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
});

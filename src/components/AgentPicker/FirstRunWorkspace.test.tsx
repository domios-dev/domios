import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { firstRunCommands, firstRunGate, firstRunSelection } from "../../lib/firstRunGate";
import { APP_SHORTCUT_ACTIONS, type AppShortcutHandlers } from "../../lib/appShortcuts";
import { useAppShortcuts } from "../../hooks/useAppShortcuts";
import type { Tab, ThemeConfig, Workspace } from "../../lib/types";
import { FIRST_RUN_GA_TAB_MARKER } from "./constants";
import { FirstRunWorkspace } from "./FirstRunWorkspace";
import { isGeneralSectionTab } from "../Sidebar/workspaceTabs";

vi.mock("../Titlebar/KeepAwakeTray", () => ({ KeepAwakeTray: () => <button>Keep awake</button> }));
vi.mock("../Titlebar/ExtensionsTray", () => ({ ExtensionsTray: () => <button>Plugins</button> }));
vi.mock("../Titlebar/AnalyticsTray", () => ({ AnalyticsTray: () => <button>Analytics</button> }));
vi.mock("../Titlebar/AccountTray", () => ({ AccountTray: () => <button>Sign in</button> }));
vi.mock("../Titlebar/SettingsMenu", () => ({ SettingsMenu: ({ onOpenSection, afterGear }: { onOpenSection: () => void; afterGear: React.ReactNode }) => <>
  <button onClick={onOpenSection}>Settings</button>{afterGear}
</> }));
vi.mock("../WebPeerPicker/WebPeerPicker", () => ({ WebPeerPicker: () => <button>Mesh hosts</button> }));

import { Titlebar } from "../Titlebar/Titlebar";
import { buildDefaultCommands } from "../CommandPalette/CommandPalette";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, __CODETERM_DEV__: false, __APP_VERSION__: "test" });
const locked = firstRunGate({ first_run_complete: false });
const open = firstRunGate({ first_run_complete: true });
const home: Tab = { id: "home", title: "Get started", description: FIRST_RUN_GA_TAB_MARKER, tab_type: "Terminal", pty_id: null, cwd: null };
const apiTab: Tab = { id: "api", title: "Linkfixture", description: "BrowserCaller", agent_type: "fixture", tab_type: "Terminal", pty_id: null, cwd: null };
const workspace: Workspace = { id: "general", name: "General", protected: true, cwd: null, tabs: [apiTab.id, home.id], active_tab_id: apiTab.id };
const tabs = { [home.id]: home, [apiTab.id]: apiTab };
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

describe("first-run gate", () => {
  it("uses the persisted choice, failing closed while the config is unknown", () => {
    for (const gate of [locked, firstRunGate(undefined), firstRunGate({})]) {
      expect(gate.complete).toBe(false);
      expect(gate.allows("app")).toBe(false);
      for (const surface of ["onboarding", "settings", "account"] as const) expect(gate.allows(surface)).toBe(true);
    }
    expect(open.allows("app")).toBe(true);
    expect(locked.allowsSettingsSection("jobs")).toBe(false);
    expect(locked.allowsSettingsSection("general")).toBe(true);
    expect(open.allowsSettingsSection("jobs")).toBe(true);
  });

  it("pins the frontend selection across an API spawn without removing any tabs", () => {
    const snapshot = { workspaces: [workspace], tabs, active_workspace_id: workspace.id,
      activeTabIds: { [workspace.id]: apiTab.id }, focused_tab_id: apiTab.id };
    expect(firstRunSelection(locked, snapshot)).toEqual({ active_workspace_id: "general", focused_tab_id: "home", activeTabIds: { general: "home" } });
    expect(Object.keys(snapshot.tabs).sort()).toEqual(["api", "home"]);
    expect(snapshot.workspaces[0].tabs).toEqual(["api", "home"]);
    expect(firstRunSelection(open, snapshot)).toBe(snapshot);
    expect(firstRunSelection(locked, { ...snapshot, tabs: { api: apiTab } }).focused_tab_id).toBeNull();
  });

  it("keeps API tabs visible and disabled while the normal app controls are unmounted", async () => {
    const closeSettings = vi.fn();
    const render = (complete: boolean) => <FirstRunWorkspace gate={complete ? open : locked} tabs={tabs}
      onGetStarted={closeSettings} onboarding={<button>Choose provider</button>}>
      <button>New workspace</button><button>New session</button><button>New tab</button><button>Mesh</button><button>Memory</button>
    </FirstRunWorkspace>;
    const { host, root } = await mount(render(false));
    const buttons = () => Array.from(host.querySelectorAll("button"));
    expect(buttons()).toHaveLength(3);
    expect(host.querySelector('button[aria-label="Get started"]')).not.toBeNull();
    const row = host.querySelector<HTMLButtonElement>('button[aria-label="Linkfixture"]')!;
    expect(row.disabled).toBe(true);
    expect(row.textContent).toContain("BrowserCaller");
    expect(host.querySelector('[aria-current="page"]')?.textContent).toContain("Get started");
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Get started");
    await act(async () => row.click());
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Get started");
    await act(async () => root.render(render(true)));
    expect(buttons().map((button) => button.textContent)).toContain("New workspace");
    expect(buttons().every((button) => !button.disabled)).toBe(true);
  });

  it("allows Settings and a return to Get started", async () => {
    const onGetStarted = vi.fn();
    const { host } = await mount(<FirstRunWorkspace gate={locked} tabs={tabs} onGetStarted={onGetStarted}
      settings={<button>Settings control</button>} onboarding={<button>Choose provider</button>}><button>New tab</button></FirstRunWorkspace>);
    expect(host.querySelector('main[aria-label="Settings"] button')?.textContent).toBe("Settings control");
    await act(async () => host.querySelector<HTMLButtonElement>('section[aria-label="General"] button')!.click());
    expect(onGetStarted).toHaveBeenCalledOnce();
  });

  it("makes creation, close, reopen, focus and paste shortcuts no-ops until completion", async () => {
    const handlers: AppShortcutHandlers = {
      new_tab: vi.fn(), close_tab: vi.fn(), reopen_tab: vi.fn(), next_tab: vi.fn(), prev_tab: vi.fn(), new_workspace: vi.fn(),
      copy: vi.fn(), paste: vi.fn(), search: vi.fn(), zoom_in: vi.fn(), zoom_out: vi.fn(), zoom_reset: vi.fn(),
      open_settings: vi.fn(), toggle_sidebar: vi.fn(),
    };
    function Shortcuts({ complete }: { complete: boolean }) {
      useAppShortcuts({}, handlers, complete ? open : locked);
      return null;
    }
    const { root } = await mount(<Shortcuts complete={false} />);
    const press = (code: string, shiftKey = false) => window.dispatchEvent(new KeyboardEvent("keydown", { code, ctrlKey: true, shiftKey, bubbles: true }));
    await act(async () => {
      press("KeyT"); press("KeyN", true); press("KeyT", true); press("KeyW"); press("Tab"); press("KeyV", true); press("Comma");
    });
    for (const action of APP_SHORTCUT_ACTIONS) expect(handlers[action]).toHaveBeenCalledTimes(action === "open_settings" ? 1 : 0);
    await act(async () => root.render(<Shortcuts complete />));
    await act(async () => { press("KeyT"); press("KeyN", true); });
    expect(handlers.new_tab).toHaveBeenCalledOnce();
    expect(handlers.new_workspace).toHaveBeenCalledOnce();
  });

  it("exposes only Settings through the command palette until the choice is saved", () => {
    const commands = buildDefaultCommands({ newTab: vi.fn(), closeTab: vi.fn(), newWorkspace: vi.fn(),
      openSettings: vi.fn(), toggleSidebar: vi.fn(), openThemeSettings: vi.fn(), spawnClaude: vi.fn() });
    expect(firstRunCommands(locked, commands).map((command) => command.id)).toEqual(["settings"]);
    expect(firstRunCommands(open, commands)).toBe(commands);
  });

  it("keeps Settings and sign-in in the titlebar while hiding other interactive trays", async () => {
    const settings = vi.fn();
    const { host } = await mount(<Titlebar gate={locked} theme={{} as ThemeConfig} onOpenSettings={settings}
      onOpenExtensions={vi.fn()} onToggleAnalytics={vi.fn()} showSidebarToggle onToggleSidebar={vi.fn()} />);
    const buttons = Array.from(host.querySelectorAll("button"));
    expect(buttons.map((button) => button.textContent)).toEqual(["Settings", "Sign in"]);
    await act(async () => buttons[0].click());
    expect(settings).toHaveBeenCalledOnce();
  });

  it("classifies General rows by role rather than workspace or agent type", () => {
    expect(isGeneralSectionTab(home)).toBe(true);
    expect(isGeneralSectionTab(apiTab)).toBe(false);
    expect(isGeneralSectionTab({ ...apiTab, protected: true })).toBe(false);
    expect(isGeneralSectionTab({ ...apiTab, role: "Manager", role_profile: "orchestrator" })).toBe(false);
    expect(isGeneralSectionTab({ ...apiTab, role: "Manager", role_profile: "general_agent" })).toBe(true);
  });

  it("does not expose the Jobs creation surface through the Settings menu", async () => {
    const { SettingsMenu } = await vi.importActual<typeof import("../Titlebar/SettingsMenu")>("../Titlebar/SettingsMenu");
    const { host } = await mount(<SettingsMenu gate={locked} onOpenSection={vi.fn()} />);
    await act(async () => host.querySelector<HTMLButtonElement>('button[title="Settings"]')!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 })));
    expect(document.querySelector('[role="menu"]:not([hidden])')).not.toBeNull();
    expect(document.querySelector('[data-testid="settings-menu-general"]')).not.toBeNull();
    expect(document.querySelector('[data-testid="settings-menu-jobs"]')).toBeNull();
  });
});

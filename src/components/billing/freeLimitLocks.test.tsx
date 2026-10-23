import { act, createElement, useRef, type ReactNode } from "react";
import type { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, it, vi } from "vitest";

const writes = vi.hoisted(() => ({ writeTerminalInput: vi.fn(async () => {}) }));
vi.mock("../../hooks/terminal/terminalInputWrite", async (actual) => ({ ...await actual<object>(), ...writes }));

import { useTerminalInput } from "../../hooks/terminal/useTerminalInput";
import { isInputBlockedTab } from "../../lib/tabInputPolicy";
import type { MultiplexerState } from "../../lib/types";
import { useMuxState } from "../../store/multiplexer";
import { TabItem } from "../Sidebar/TabItem";
import { TeamUnit } from "../Sidebar/TeamUnit";
import { SidebarDnd } from "../Sidebar/sortable";
import { WorkspaceCardGrid } from "../Sidebar/WorkspaceCardGrid";
import { TabLockMark } from "../Terminal/TabLockMark";
import { WorkspaceItem } from "../Sidebar/WorkspaceItem";
import { ChatComposerSurface } from "../Terminal/ChatComposerSurface";
import { LockedOverlay } from "./LockBadge";
import { openLockedPlans } from "./freeLimitGate";
import { fakeDaemon, mountApp, modal, modalLine, seedState, setLocked, unmountAll, WORKSPACE } from "./freeLimitHarness";
import { lockedView, tabLockedView, type FreeLimits } from "./freeLimitModel";
import { useTabLock } from "./useTabLock";

const FREE: FreeLimits = { enforcing: true, max: { workspaces: 2, teams: 6, "agent.tabs": 35 } };
const PAID_LIMITS = { enforcing: true, workspaces: null, teams: null, "agent.tabs": null };
const WORKSPACES = "The free version includes 2 workspaces.";
const TEAMS = "The free version includes 6 Teams across all workspaces.";
const TABS = "The free version allows 35 tabs running at once.";

afterEach(async () => {
  writes.writeTerminalInput.mockClear();
  await unmountAll();
});

const nameOf = (element: Element) => element.getAttribute("aria-label") ?? element.textContent ?? "";
const buttons = (name: RegExp) => Array.from(document.querySelectorAll("button")).filter((button) => name.test(nameOf(button)));
const lockMarks = () => Array.from(document.querySelectorAll("[role=img]")).filter((mark) => /^Locked: free plan limit$/.test(nameOf(mark)));
const click = (element: Element) => act(async () => { element.dispatchEvent(new MouseEvent("click", { bubbles: true })); });

/** Real rows over the store's state; `selected` records what a normal (unlocked) click would have navigated to. */
function Rows({ selected }: { selected: string[] }) {
  const state = useMuxState();
  const noop = () => {};
  return createElement("div", null,
    ...state.workspaces.filter((workspace) => !workspace.protected).map((workspace) => createElement(WorkspaceItem, {
      key: workspace.id, workspace, isActive: false, isExpanded: false, status: null, canClose: true,
      onClick: () => selected.push(workspace.id), onToggleExpand: noop, onClose: noop, onRename: noop,
      onNewTerminal: noop, onNewSession: noop, onNewAgent: noop,
    })),
    ...Object.values(state.tabs).map((tab, index) => createElement(TabItem, {
      key: tab.id, tab, index, isActive: false, canClose: true, workspaceId: WORKSPACE,
      onClick: () => selected.push(tab.id), onClose: noop,
    })),
    ...(state.teams ?? []).map((team) => createElement(TeamUnit, {
      key: team.id, team, plan: null, tabs: state.tabs, managers: null, managerCount: 0, workers: null, workerCount: 0,
      workingWorkerCount: 0, providers: [], onOpenEffort: () => selected.push(team.id),
    })),
  );
}

async function mountRows(state: MultiplexerState, extra: ReactNode = null) {
  const selected: string[] = [];
  const app = await mountApp(fakeDaemon(state, FREE), async () => {}, createElement("div", null, createElement(Rows, { selected }), extra));
  return { app, selected };
}

const seeded = () => seedState({ workspaces: 3, teams: 2, tabs: 3 });

describe("locked items stay listed with a lock affordance", () => {
  it("disables mutation controls in the real locked sidebar menu", async () => {
    const state = seeded();
    setLocked(state, ["tab-0"], true);
    await mountRows(state);
    const row = buttons(/^Open tab tab-0$/)[0];
    await act(async () => { row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true })); });
    const rename = document.querySelector<HTMLButtonElement>("[data-testid=tab-menu-rename]");
    expect(rename).not.toBeNull();
    expect(rename?.disabled).toBe(true);
    expect(rename?.title).toBe("Tab locked by the free plan");
    const close = document.querySelector<HTMLButtonElement>("[data-testid=tab-menu-close]");
    expect(close).not.toBeNull();
    expect(close?.disabled).toBe(false);
  });

  it("renders a lock mark on exactly the locked workspace, tab, and Team, without dropping any row", async () => {
    const state = seeded();
    setLocked(state, ["ws-1", "tab-0", "team-0"], true);
    await mountRows(state);
    expect(lockMarks().map(nameOf)).toEqual(["Locked: free plan limit", "Locked: free plan limit", "Locked: free plan limit"]);
    expect(lockMarks().map((mark) => mark.getAttribute("title")).sort()).toEqual([
      "Tab locked by the free plan. Upgrade to unlock.",
      "Team locked by the free plan. Upgrade to unlock.",
      "Workspace locked by the free plan. Upgrade to unlock.",
    ]);
    expect(document.querySelectorAll("[data-testid=workspace-item]")).toHaveLength(3);
    expect(document.querySelectorAll("[data-testid=team-unit]")).toHaveLength(2);
    expect(buttons(/^Open tab /)).toHaveLength(3);
  });

  it("marks nothing while no item is locked", async () => {
    await mountRows(seeded());
    expect(lockMarks()).toHaveLength(0);
    expect(buttons(/^Open tab /)).toHaveLength(3);
  });
});

/** The workspace tiles and the tab header, which read the same lock marks as the sidebar rows. */
function Surfaces() {
  const state = useMuxState();
  const ordinary = state.workspaces.filter((workspace) => !workspace.protected);
  return createElement(SidebarDnd, { onReorder: () => {} } as never,
    createElement(WorkspaceCardGrid, {
      workspaces: ordinary, selectedWorkspaceId: null, statuses: {}, sortIds: ordinary.map((workspace) => workspace.id),
      onSelect: () => {}, onCreate: () => {},
    }),
    createElement(TabLockMark, { tab: state.tabs["tab-0"] ?? null }));
}

describe("every surface marks a locked item and clears live", () => {
  const tiles = () => Array.from(document.querySelectorAll("[data-testid=workspace-card]"));
  const marksIn = (element: Element) => Array.from(element.querySelectorAll("[role=img]")).map(nameOf).filter((name) => name === "Locked: free plan limit");

  it("marks the locked workspace's tile and the locked tab's header, and none of the unlocked ones", async () => {
    const state = seeded();
    setLocked(state, ["ws-1", "tab-0"], true);
    await mountRows(state, createElement(Surfaces));
    expect(tiles().filter((tile) => marksIn(tile).length === 1).map((tile) => tile.getAttribute("data-workspace-id"))).toEqual(["ws-1"]);
    expect(tiles().filter((tile) => marksIn(tile).length === 0)).toHaveLength(2);
    expect(lockMarks().length).toBe(4);
  });

  it("shows no mark anywhere while nothing is locked", async () => {
    await mountRows(seeded(), createElement(Surfaces));
    expect(lockMarks()).toHaveLength(0);
  });

  it("clears the tile and header marks on the entitlement change event", async () => {
    const state = seeded();
    setLocked(state, ["ws-1", "tab-0"], true);
    const { app } = await mountRows(state, createElement(Surfaces));
    expect(lockMarks().length).toBe(4);
    setLocked(state, ["ws-1", "tab-0"], false);
    await app.emit("entitlement-changed", { limits: PAID_LIMITS, locks: { workspaces: [], teams: [], "agent.tabs": [] } });
    expect(lockMarks()).toHaveLength(0);
  });
});

describe("a click on a locked item opens the plans modal", () => {
  it.each([
    ["workspace row", ["ws-1"], /^Open workspace Workspace 1$/, WORKSPACES],
    ["tab row", ["tab-1"], /^Open tab tab-1$/, TABS],
    ["Team", ["team-0"], /^Open task status for Team$/, TEAMS],
  ] as const)("%s", async (_name, ids, control, line) => {
    const state = seeded();
    setLocked(state, ids, true);
    const { selected } = await mountRows(state);
    const [target] = buttons(control);
    expect(target).toBeDefined();
    await click(target);
    expect(modal()).not.toBeNull();
    expect(modalLine()).toBe(line);
    expect(selected).toEqual([]);
  });

  it("an unlocked sibling still selects and opens no modal", async () => {
    const state = seeded();
    setLocked(state, ["ws-1"], true);
    const { selected } = await mountRows(state);
    await click(buttons(/^Open workspace Workspace 2$/)[0]);
    expect(selected).toEqual(["ws-2"]);
    expect(modal()).toBeNull();
  });
});

describe("a locked tab takes no input", () => {
  it("the composer is replaced by a disabled one and its submit never runs", async () => {
    const state = seeded();
    setLocked(state, ["tab-0"], true);
    const onSubmit = vi.fn();
    await mountRows(state, <Composer onSubmit={onSubmit} />);
    expect(Array.from(document.querySelectorAll("textarea")).map((box) => box.disabled)).toEqual([true]);
    expect(buttons(/^Send$/)).toHaveLength(0);
    await click(buttons(/View plans to unlock/)[0]);
    expect(modalLine()).toBe(TABS);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("terminal keystrokes are not forwarded for the locked tab, only for its unlocked sibling", async () => {
    const state = seeded();
    setLocked(state, ["tab-0"], true);
    const locked = fakeTerminal();
    const open = fakeTerminal();
    await mountRows(state, createElement("div", null,
      createElement(TerminalProbe, { id: "tab-0", terminal: locked }),
      createElement(TerminalProbe, { id: "tab-1", terminal: open })));
    await locked.emit("a");
    await locked.emit("[0n");
    expect(writes.writeTerminalInput).not.toHaveBeenCalled();
    await open.emit("b");
    expect(writes.writeTerminalInput).toHaveBeenCalledWith("tab-1", "pty-1", "b");
  });

  it("the overlay over locked content opens the plans modal", async () => {
    const view = lockedView("agent.tabs", { locked: true });
    await mountRows(seeded(), createElement(LockedOverlay, { view, onActivate: () => { openLockedPlans(view); } }));
    await click(buttons(/View plans to unlock/)[0]);
    expect(modalLine()).toBe(TABS);
  });

  it("only a lock or a Curator round blocks input", () => {
    const open = { locked: false } as const;
    const locked = lockedView("agent.tabs", { locked: true });
    expect(isInputBlockedTab({ role_profile: undefined }, locked)).toBe(true);
    expect(isInputBlockedTab({ role_profile: undefined }, open)).toBe(false);
    expect(isInputBlockedTab({ role_profile: "curator" }, open)).toBe(true);
    expect(isInputBlockedTab(undefined, open)).toBe(false);
  });
});

describe("a locked workspace locks the tabs inside it", () => {
  it("derives the tab lock from the workspace in the pure model", () => {
    expect(tabLockedView({}, { locked: true })).toMatchObject({ locked: true, limit: "workspaces" });
    expect(tabLockedView({ locked: true }, {})).toMatchObject({ locked: true, limit: "agent.tabs" });
    expect(tabLockedView({ locked: true }, { locked: true })).toMatchObject({ limit: "agent.tabs" });
    expect(tabLockedView({}, { locked: false })).toEqual({ locked: false });
    expect(tabLockedView(undefined, undefined)).toEqual({ locked: false });
  });

  const lockedWorkspace = () => {
    const state = seeded();
    setLocked(state, [WORKSPACE], true);
    return state;
  };

  it("marks every tab row, lists them all, and a click opens the workspaces modal", async () => {
    const { selected } = await mountRows(lockedWorkspace());
    expect(lockMarks().map((mark) => mark.getAttribute("title") ?? "").filter((title) => /^Tab locked: its workspace/.test(title))).toHaveLength(3);
    expect(buttons(/^Open tab /)).toHaveLength(3);
    await click(buttons(/^Open tab tab-2$/)[0]);
    expect(modalLine()).toBe(WORKSPACES);
    expect(selected).toEqual([]);
  });

  it("makes the composer locked and drops terminal keystrokes though the daemon did not mark the tab", async () => {
    const terminal = fakeTerminal();
    const state = lockedWorkspace();
    expect(state.tabs["tab-0"].locked).toBeUndefined();
    await mountRows(state, <div><Composer onSubmit={() => {}} /><TerminalProbe id="tab-0" terminal={terminal} /></div>);
    expect(document.querySelectorAll("textarea[disabled]")).toHaveLength(1);
    await terminal.emit("a");
    expect(writes.writeTerminalInput).not.toHaveBeenCalled();
  });

  it("unlocks the tabs when the workspace unlocks", async () => {
    const state = lockedWorkspace();
    const terminal = fakeTerminal();
    const { app } = await mountRows(state, <div><Composer onSubmit={() => {}} /><TerminalProbe id="tab-0" terminal={terminal} /></div>);
    setLocked(state, [WORKSPACE], false);
    await app.emit("entitlement-changed", { limits: PAID_LIMITS, locks: { workspaces: [], teams: [], "agent.tabs": [] } });
    expect(lockMarks()).toHaveLength(0);
    expect(document.querySelectorAll("textarea[disabled]")).toHaveLength(0);
    await terminal.emit("b");
    expect(writes.writeTerminalInput).toHaveBeenCalledWith("tab-0", "pty-1", "b");
  });
});

describe("paying unlocks live", () => {
  it("inherits the Team lock on tab rows and composers and clears it live", async () => {
    const state = seeded();
    state.tabs["tab-0"].team_id = "team-0";
    setLocked(state, ["team-0"], true);
    const { app, selected } = await mountRows(state, <Composer onSubmit={() => {}} />);
    expect(state.tabs["tab-0"].locked).toBeUndefined();
    expect(document.querySelector('[data-testid="locked-composer"]')).not.toBeNull();
    const row = buttons(/^Open tab tab-0$/)[0];
    expect(row.querySelector('[aria-label="Locked: free plan limit"]')).not.toBeNull();
    await click(row);
    expect(modalLine()).toBe(TEAMS);
    expect(selected).toEqual([]);
    setLocked(state, ["team-0"], false);
    await app.emit("entitlement-changed", { limits: PAID_LIMITS, locks: { workspaces: [], teams: [], "agent.tabs": [] } });
    expect(document.querySelector('[data-testid="locked-composer"]')).toBeNull();
    expect(row.querySelector('[aria-label="Locked: free plan limit"]')).toBeNull();
  });

  it("the entitlement change event clears every lock, enables the composer, forwards input, and a click selects", async () => {
    const state = seeded();
    setLocked(state, ["ws-1", "tab-0", "team-0"], true);
    const terminal = fakeTerminal();
    const { app, selected } = await mountRows(state, createElement("div", null,
      <Composer onSubmit={() => {}} />, createElement(TerminalProbe, { id: "tab-0", terminal })));
    expect(lockMarks()).toHaveLength(3);
    expect(document.querySelectorAll("textarea[disabled]")).toHaveLength(1);
    await terminal.emit("x");
    expect(writes.writeTerminalInput).not.toHaveBeenCalled();

    setLocked(state, ["ws-1", "tab-0", "team-0"], false);
    await app.emit("entitlement-changed", { limits: PAID_LIMITS, locks: { workspaces: [], teams: [], "agent.tabs": [] } });

    expect(lockMarks()).toHaveLength(0);
    expect(document.querySelectorAll("textarea[disabled]")).toHaveLength(0);
    expect(buttons(/^Send$/)).toHaveLength(1);
    await terminal.emit("y");
    expect(writes.writeTerminalInput).toHaveBeenCalledWith("tab-0", "pty-1", "y");
    await click(buttons(/^Open workspace Workspace 1$/)[0]);
    expect(selected).toEqual(["ws-1"]);
    expect(modal()).toBeNull();
  });
});

/** Stand-in for ChatComposer: the real one mounts only while its tab is not locked. */
function Composer({ onSubmit }: { onSubmit: () => void }) {
  const tab = useMuxState().tabs["tab-0"];
  const lock = useTabLock(tab);
  return (
    <ChatComposerSurface view={{ kind: "none" }} tab={tab} lock={lock}>
      <form onSubmit={(event) => { event.preventDefault(); onSubmit(); }}>
        <textarea aria-label="Message composer" />
        <button type="submit">Send</button>
      </form>
    </ChatComposerSurface>
  );
}

function fakeTerminal() {
  let onData: (data: string) => void = () => { throw new Error("onData not registered"); };
  const term = {
    attachCustomKeyEventHandler() {},
    onData(handler: (data: string) => void) { onData = handler; return { dispose() {} }; },
  } as unknown as Terminal;
  return { term, emit: (data: string) => act(async () => onData(data)) };
}

/** The real input hook over a fake xterm, gated by the live tab exactly as TerminalTab gates it. */
function TerminalProbe({ id, terminal }: { id: string; terminal: ReturnType<typeof fakeTerminal> }) {
  const tab = useMuxState().tabs[id];
  const lock = useTabLock(tab);
  const termRef = useRef(terminal.term);
  const connectedPtyRef = useRef<string | null>("pty-1");
  const containerRef = useRef<HTMLDivElement | null>(null);
  useTerminalInput({
    tabId: id, termRef, connectedPtyRef, containerRef, termReady: true, terminalActive: true,
    readOnly: isInputBlockedTab(tab, lock), onTabKey: () => false, onNonTabKey() {}, onInputData() {},
  });
  return null;
}

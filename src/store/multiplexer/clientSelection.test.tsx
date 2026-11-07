import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { MultiplexerState, Tab, Workspace } from "../../lib/types";
import type { MultiplexerActionsContextType } from "./context";
import type { ExtendedState } from "./tree";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// The native desktop client: the one that used to follow every other client's selection.
vi.hoisted(() => {
  (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
});

type Bridge = typeof import("../../lib/bridge");
type Store = typeof import("./index");
type Handler = (event: { payload: unknown }) => void;

let bridge: Bridge;
let store: Store;

beforeAll(async () => {
  bridge = await import("../../lib/bridge");
  store = await import("./index");
});

beforeEach(() => {
  window.sessionStorage.clear();
  window.localStorage.clear();
});

function tab(id: string): Tab {
  return { id, pty_id: null, title: id, cwd: null } as unknown as Tab;
}

function workspace(id: string, tabs: string[], activeTabId: string): Workspace {
  return { id, name: id, tabs, active_tab_id: activeTabId, cwd: null, protected: false } as unknown as Workspace;
}

function snapshot(generation: number, activeWorkspaceId: string, active: Record<string, string>): MultiplexerState {
  const workspaces = [workspace("ws-a", ["a1", "a2"], active["ws-a"] ?? "a1"), workspace("ws-b", ["b1"], "b1")];
  const tabs = Object.fromEntries(workspaces.flatMap((ws) => ws.tabs.map((id) => [id, tab(id)])));
  return { workspaces, tabs, teams: [], active_workspace_id: activeWorkspaceId, focused_tab_id: null, generation } as unknown as MultiplexerState;
}

/** One daemon shared by every client; `chatty` replays the old broadcast of a selection to every client. */
function createDaemon(options: { chatty: boolean }) {
  const listeners = new Map<string, Set<Handler>>();
  const daemon = {
    state: snapshot(1, "ws-a", { "ws-a": "a1" }),
    commands: [] as string[],
    emit(event: string, payload: unknown) {
      for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
    },
    invoke: async <T,>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
      daemon.commands.push(cmd);
      if (cmd === "mux_get_state") return daemon.state as T;
      if (cmd === "ordinary_tabs_list" || cmd === "list_notifications") return [] as T;
      if (cmd === "mux_set_active_tab_and_workspace" && options.chatty) {
        const workspaceId = String(args.workspaceId);
        const tabId = String(args.tabId);
        daemon.state = snapshot(daemon.state.generation! + 1, workspaceId, { [workspaceId]: tabId });
        daemon.emit("mux-state-changed", daemon.state);
        daemon.emit("tab-focused", { tab_id: tabId });
      }
      return undefined as T;
    },
    listen: async <T,>(event: string, handler: (event: { payload: T }) => void) => {
      const set = listeners.get(event) ?? new Set<Handler>();
      set.add(handler as Handler);
      listeners.set(event, set);
      return () => set.delete(handler as Handler);
    },
  };
  return daemon;
}

type Selection = [string | null, string | null, string | null];

function selectionOf(state: ExtendedState): Selection {
  const workspaceId = state.active_workspace_id;
  return [workspaceId, workspaceId ? state.activeTabIds[workspaceId] ?? null : null, state.focused_tab_id];
}

const mounted: Root[] = [];

async function settle(turns = 40) {
  for (let i = 0; i < turns; i++) await act(async () => { await Promise.resolve(); });
}

async function mountClient() {
  const holder: { state: ExtendedState | null; actions: MultiplexerActionsContextType | null; changes: Selection[] } = { state: null, actions: null, changes: [] };
  function Probe() {
    const state = store.useMuxState() as ExtendedState;
    holder.state = state;
    holder.actions = store.useMultiplexer();
    const next = selectionOf(state);
    const last = holder.changes[holder.changes.length - 1];
    if (!last || last.some((value, index) => value !== next[index])) holder.changes.push(next);
    return null;
  }
  const root = createRoot(document.createElement("div"));
  mounted.push(root);
  await act(async () => root.render(createElement(store.MultiplexerProvider, null, createElement(Probe))));
  await settle();
  return { selection: () => selectionOf(holder.state!), actions: () => holder.actions!, changes: () => holder.changes.length };
}

async function mountClients(daemon: ReturnType<typeof createDaemon>) {
  bridge.__setInvokeForTest(daemon.invoke);
  bridge.__setListenForTest(daemon.listen);
  const a = await mountClient();
  const b = await mountClient();
  return { a, b };
}

afterEach(async () => {
  for (const root of mounted.splice(0)) await act(async () => root.unmount());
  bridge.__setInvokeForTest(null);
  bridge.__setListenForTest(null);
});

describe("each client keeps its own selection", () => {
  it("a selection in one client never moves, reads or wakes another client", async () => {
    const daemon = createDaemon({ chatty: true });
    const { a, b } = await mountClients(daemon);
    expect(b.selection()[0]).toBe("ws-a");
    expect(b.selection()[1]).toBe("a1");
    const before = b.selection();
    const changesBefore = b.changes();
    daemon.commands.length = 0;

    await act(async () => { a.actions().setActiveTab("ws-b", "b1"); });
    await settle();

    expect(a.selection()).toEqual(["ws-b", "b1", "b1"]);
    expect(b.selection()).toEqual(before);
    expect(b.changes()).toBe(changesBefore);
    expect(daemon.commands).toEqual(["mux_set_active_tab_and_workspace"]);
  });

  it("the client that acted shows its tab even when the daemon broadcasts nothing", async () => {
    const daemon = createDaemon({ chatty: false });
    const { a, b } = await mountClients(daemon);
    const before = b.selection();

    await act(async () => { a.actions().setActiveTab("ws-b", "b1"); });
    await settle();

    expect(a.selection()).toEqual(["ws-b", "b1", "b1"]);
    expect(b.selection()).toEqual(before);
  });

  it("an explicit focus request shows its tab in every client", async () => {
    const daemon = createDaemon({ chatty: false });
    const { a, b } = await mountClients(daemon);

    await act(async () => { daemon.emit("tab-focus-requested", { tab_id: "b1", workspace_id: "ws-b" }); });
    await settle();

    expect(a.selection()).toEqual(["ws-b", "b1", "b1"]);
    expect(b.selection()).toEqual(["ws-b", "b1", "b1"]);
  });

  it("an explicit workspace request opens that workspace in every client at its own tab there", async () => {
    const daemon = createDaemon({ chatty: false });
    const { a, b } = await mountClients(daemon);
    await act(async () => { a.actions().setActiveTab("ws-a", "a2"); });
    await act(async () => { a.actions().setActiveTab("ws-b", "b1"); });

    await act(async () => { daemon.emit("tab-focus-requested", { tab_id: null, workspace_id: "ws-a" }); });
    await settle();

    expect(a.selection()).toEqual(["ws-a", "a2", "a2"]);
    expect(b.selection()).toEqual(["ws-a", "a1", "a1"]);
  });
});

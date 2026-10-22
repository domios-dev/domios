import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import type { MultiplexerState, Tab, Workspace } from "../../lib/types";
import type { MultiplexerActionsContextType } from "./context";
import type { ExtendedState } from "./tree";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Bridge = typeof import("../../lib/bridge");
type Store = typeof import("./index");
type Handler = (event: { payload: unknown }) => void;

let bridge: Bridge;
let store: Store;

beforeAll(async () => {
  bridge = await import("../../lib/bridge");
  store = await import("./index");
});

function tab(id: string): Tab {
  return { id, pty_id: null, title: id, cwd: null } as unknown as Tab;
}

function snapshot(generation: number, workspaces: Workspace[], activeWorkspaceId: string): MultiplexerState {
  const tabs = Object.fromEntries(workspaces.flatMap((workspace) => workspace.tabs.map((id) => [id, tab(id)])));
  return { workspaces, tabs, teams: [], active_workspace_id: activeWorkspaceId, focused_tab_id: null, generation } as unknown as MultiplexerState;
}

function workspace(id: string, tabs: string[], activeTabId: string): Workspace {
  return { id, name: id, tabs, active_tab_id: activeTabId, cwd: null, protected: false } as unknown as Workspace;
}

/** A daemon whose state the test scripts; `beforeRead` runs inside each state read, before it answers. */
function createDaemon(initial: MultiplexerState) {
  const listeners = new Map<string, Set<Handler>>();
  const daemon = {
    state: initial,
    beforeRead: null as (() => void) | null,
    created: null as ((args: Record<string, unknown>) => unknown) | null,
    emit(event: string, payload: unknown) {
      for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
    },
    invoke: async <T,>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
      if (cmd === "mux_get_state") {
        const hook = daemon.beforeRead;
        daemon.beforeRead = null;
        hook?.();
        return daemon.state as T;
      }
      if ((cmd === "mux_create_workspace" || cmd === "mux_create_tab") && daemon.created) return daemon.created(args) as T;
      if (cmd === "ordinary_tabs_list" || cmd === "list_notifications") return [] as T;
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

const mounted: Root[] = [];

async function settle(turns = 40) {
  for (let i = 0; i < turns; i++) await act(async () => { await Promise.resolve(); });
}

async function mountPage(daemon: ReturnType<typeof createDaemon>) {
  bridge.__setInvokeForTest(daemon.invoke);
  bridge.__setListenForTest(daemon.listen);
  const holder: { state: ExtendedState | null; actions: MultiplexerActionsContextType | null } = { state: null, actions: null };
  function Probe() {
    holder.state = store.useMuxState() as ExtendedState;
    holder.actions = store.useMultiplexer();
    return null;
  }
  const root = createRoot(document.createElement("div"));
  mounted.push(root);
  await act(async () => root.render(createElement(store.MultiplexerProvider, null, createElement(Probe))));
  await settle();
  return { state: () => holder.state!, actions: () => holder.actions! };
}

afterEach(async () => {
  for (const root of mounted.splice(0)) await act(async () => root.unmount());
  bridge.__setInvokeForTest(null);
  bridge.__setListenForTest(null);
});

describe("the page selection follows what the page creates", () => {
  it("selects a newly created workspace", async () => {
    const daemon = createDaemon(snapshot(1, [workspace("ws-a", ["a1"], "a1")], "ws-a"));
    const page = await mountPage(daemon);
    expect(page.state().active_workspace_id).toBe("ws-a");

    daemon.created = () => {
      daemon.state = snapshot(2, [workspace("ws-a", ["a1"], "a1"), workspace("ws-b", ["b1"], "b1")], "ws-b");
      return { id: "ws-b", name: "ws-b" };
    };
    await act(async () => { await page.actions().createWorkspace("ws-b", "/repo/b"); });

    expect(page.state().active_workspace_id).toBe("ws-b");
    expect(page.state().activeTabIds["ws-b"]).toBe("b1");
  });

  it("selects a new Tab even when an older snapshot without it lands mid-create", async () => {
    const daemon = createDaemon(snapshot(1, [workspace("ws-a", ["a1"], "a1")], "ws-a"));
    const page = await mountPage(daemon);

    daemon.created = () => {
      daemon.state = snapshot(3, [workspace("ws-a", ["a1", "a2"], "a2")], "ws-a");
      // Generated before the create, delivered after it: newer than the page, older than the Tab.
      daemon.beforeRead = () => daemon.emit("mux-state-changed", snapshot(2, [workspace("ws-a", ["a1"], "a1")], "ws-a"));
      return { id: "a2", title: "a2" };
    };
    await act(async () => { await page.actions().createTab("ws-a"); });
    await settle();

    expect(page.state().active_workspace_id).toBe("ws-a");
    expect(page.state().activeTabIds["ws-a"]).toBe("a2");
  });
});

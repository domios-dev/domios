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
let saveClientPageSelection: typeof import("../../lib/clientPageSelection").saveClientPageSelection;

beforeAll(async () => {
  bridge = await import("../../lib/bridge");
  store = await import("./index");
  ({ saveClientPageSelection } = await import("../../lib/clientPageSelection"));
});

const tab = (id: string) => ({ id, pty_id: null, title: id, cwd: null }) as unknown as Tab;
const workspace = (id: string, tabs: string[]) => ({ id, name: id, tabs, active_tab_id: tabs[0], cwd: null, protected: false }) as unknown as Workspace;

function snapshot(): MultiplexerState {
  const workspaces = [workspace("ws-a", ["a1", "a2"]), workspace("ws-b", ["b1"])];
  const tabs = Object.fromEntries(workspaces.flatMap((entry) => entry.tabs.map((id) => [id, tab(id)])));
  return { workspaces, tabs, teams: [], active_workspace_id: "ws-a", focused_tab_id: null, generation: 3 } as unknown as MultiplexerState;
}

/** A daemon whose first state read stays pending until the test lets the restore land. */
function createDaemon() {
  const listeners = new Map<string, Set<Handler>>();
  let land!: () => void;
  const read = new Promise<MultiplexerState>((resolve) => { land = () => resolve(snapshot()); });
  return {
    land: () => land(),
    invoke: async <T,>(cmd: string): Promise<T> => {
      if (cmd === "mux_get_state") return await read as T;
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
  window.localStorage.clear();
});

describe("a reload restore lands behind the user's navigation", () => {
  it("restores the saved workspace and tab when the user has not navigated", async () => {
    saveClientPageSelection({ workspaceId: "ws-b", tabId: "b1" });
    const daemon = createDaemon();
    const page = await mountPage(daemon);
    daemon.land();
    await settle();
    expect(page.state().active_workspace_id).toBe("ws-b");
    expect(page.state().activeTabIds["ws-b"]).toBe("b1");
  });

  it("keeps what the user opened right after the reload when the restore completes later", async () => {
    saveClientPageSelection({ workspaceId: "ws-b", tabId: "b1" });
    const daemon = createDaemon();
    const page = await mountPage(daemon);
    await act(async () => page.actions().setActiveTab("ws-a", "a2"));
    daemon.land();
    await settle();
    expect(page.state().active_workspace_id).toBe("ws-a");
    expect(page.state().activeTabIds["ws-a"]).toBe("a2");
    expect(page.state().focused_tab_id).toBe("a2");
  });
});

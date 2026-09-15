import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import type { MultiplexerState, Tab, Workspace } from "../../lib/types";
import type { ExtendedState } from "./tree";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// Desktop webview: IS_WEB is derived from this marker when the bridge loads.
(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

const SNAPSHOT_BUSY = "state snapshot unavailable while tab state is initializing or changing";
const GENERAL_ID = "bfb90821";
const MAIN_ID = "46137477";
const ORDINARY_ID = "ws-ordinary";
const ORDINARY_TAB_ID = "tab-ordinary";

type Bridge = typeof import("../../lib/bridge");
type Store = typeof import("./index");
type Handler = (event: { payload: unknown }) => void;

let bridge: Bridge;
let store: Store;
let partitionSidebarWorkspaces: typeof import("../../components/Sidebar/sidebarSections").partitionSidebarWorkspaces;
let tabsInWorkspace: typeof import("../../components/Sidebar/workspaceTabs").tabsInWorkspace;
let sortByOrder: typeof import("../../components/Sidebar/sortable").sortByOrder;
let isGeneralAgent: typeof import("../../lib/roleKind").isGeneralAgent;

beforeAll(async () => {
  ({ isGeneralAgent } = await import("../../lib/roleKind"));
  bridge = await import("../../lib/bridge");
  store = await import("./index");
  ({ partitionSidebarWorkspaces } = await import("../../components/Sidebar/sidebarSections"));
  ({ tabsInWorkspace } = await import("../../components/Sidebar/workspaceTabs"));
  ({ sortByOrder } = await import("../../components/Sidebar/sortable"));
});

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function snapshot(generation: number, options: { withGeneral?: boolean; mainTitle?: string } = {}): MultiplexerState {
  const withGeneral = options.withGeneral ?? true;
  const workspaces = [
    ...(withGeneral
      ? [{ id: GENERAL_ID, name: "General", tabs: [MAIN_ID], active_tab_id: MAIN_ID, cwd: null, protected: true, sort_order: 0 }]
      : []),
    { id: ORDINARY_ID, name: "codeterm", tabs: [ORDINARY_TAB_ID], active_tab_id: ORDINARY_TAB_ID, cwd: null, protected: false, sort_order: 1 },
  ] as Workspace[];
  const tabs: Record<string, Tab> = {
    [ORDINARY_TAB_ID]: { id: ORDINARY_TAB_ID, pty_id: null, title: "shell", cwd: null } as unknown as Tab,
  };
  if (withGeneral) {
    tabs[MAIN_ID] = {
      id: MAIN_ID,
      pty_id: null,
      title: options.mainTitle ?? "Main",
      cwd: null,
      protected: true,
      role: "Manager",
      role_profile: "general_agent",
      agent_type: "codex",
    } as unknown as Tab;
  }
  return {
    workspaces,
    tabs,
    teams: [],
    active_workspace_id: withGeneral ? GENERAL_ID : ORDINARY_ID,
    focused_tab_id: null,
    generation,
  } as unknown as MultiplexerState;
}

/** Scripted daemon: every read and every mux-state-changed bind stays pending
 * until the test settles it, and an event emitted before a bind is lost. */
function createBackend() {
  const reads: Array<Deferred<MultiplexerState>> = [];
  const binds: Array<Deferred<void>> = [];
  const listeners = new Map<string, Set<Handler>>();
  let muxReads = 0;
  return {
    invoke: async <T,>(cmd: string): Promise<T> => {
      if (cmd === "mux_get_state") {
        muxReads++;
        const read = deferred<MultiplexerState>();
        reads.push(read);
        return read.promise as Promise<T>;
      }
      if (cmd === "ordinary_tabs_list" || cmd === "list_notifications") return [] as T;
      return undefined as T;
    },
    listen: async <T,>(event: string, handler: (event: { payload: T }) => void) => {
      if (event === "mux-state-changed") {
        const bind = deferred<void>();
        binds.push(bind);
        await bind.promise;
      }
      const set = listeners.get(event) ?? new Set<Handler>();
      set.add(handler as Handler);
      listeners.set(event, set);
      return () => {
        set.delete(handler as Handler);
      };
    },
    bindMuxListeners() {
      for (const bind of binds.splice(0)) bind.resolve();
    },
    emit(event: string, payload: unknown = null) {
      for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
    },
    takeRead(): Deferred<MultiplexerState> {
      const read = reads.shift();
      if (!read) throw new Error("no mux_get_state read is in flight");
      return read;
    },
    get muxReads() {
      return muxReads;
    },
  };
}

type Backend = ReturnType<typeof createBackend>;

async function settle(turns = 30): Promise<void> {
  for (let i = 0; i < turns; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function settleUntil(condition: () => boolean, turns = 200): Promise<void> {
  for (let i = 0; i < turns; i++) {
    if (condition()) return;
    await act(async () => {
      await Promise.resolve();
    });
  }
  throw new Error(`condition not met within ${turns} microtask turns`);
}

const mounted: Array<{ root: Root; host: HTMLElement }> = [];

async function mountPage(backend: Backend) {
  bridge.__setInvokeForTest(backend.invoke);
  bridge.__setListenForTest(backend.listen);
  const holder: { state: ExtendedState | null } = { state: null };
  function Probe() {
    holder.state = store.useMuxState() as ExtendedState;
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => {
    root.render(createElement(store.MultiplexerProvider, null, createElement(Probe)));
  });
  return {
    root,
    state: () => {
      if (!holder.state) throw new Error("mux state not rendered");
      return holder.state;
    },
  };
}

function sidebarModel(state: ExtendedState) {
  const { general, workspaces } = partitionSidebarWorkspaces(sortByOrder(state.workspaces));
  return {
    general,
    generalRows: general
      ? tabsInWorkspace(general.tabs, state.tabs).map((tab) => ({
          id: tab.id,
          roleProfile: tab.role_profile,
          generalAgent: isGeneralAgent(tab),
          protected: tab.protected,
          title: tab.title,
        }))
      : [],
    ordinaryIds: workspaces.map((workspace) => workspace.id),
  };
}

function expectGeneralSurvives(state: ExtendedState, mainTitle = "Main") {
  const model = sidebarModel(state);
  expect(model.general).toMatchObject({ id: GENERAL_ID, protected: true, tabs: [MAIN_ID] });
  expect(model.generalRows).toEqual([
    { id: MAIN_ID, roleProfile: "general_agent", generalAgent: true, protected: true, title: mainTitle },
  ]);
  expect(model.ordinaryIds).toEqual([ORDINARY_ID]);
}

async function hydrate(backend: Backend) {
  await act(async () => backend.bindMuxListeners());
  await settleUntil(() => backend.muxReads > 0);
}

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  bridge.__setInvokeForTest(null);
  bridge.__setListenForTest(null);
});

describe("webview reload rehydration", () => {
  it("issues the first authoritative read only after the mux event listener is bound", async () => {
    const backend = createBackend();
    const page = await mountPage(backend);
    await settle();
    expect(backend.muxReads).toBe(0);

    backend.emit("mux-state-changed", snapshot(3));
    await settle();
    expect(page.state().workspaces).toEqual([]);

    await hydrate(backend);
    expect(backend.muxReads).toBe(1);
    await act(async () => backend.takeRead().resolve(snapshot(4)));
    await settle();
    expectGeneralSurvives(page.state());
  });

  it("keeps the protected General workspace when a reload replaces the readiness generation mid-read", async () => {
    const backend = createBackend();
    const replacedPage = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve(snapshot(10)));
    await settle();
    expectGeneralSurvives(replacedPage.state());

    backend.emit("mux-state-changed");
    await settleUntil(() => backend.muxReads === 2);
    const replacedRead = backend.takeRead();
    await act(async () => replacedPage.root.unmount());

    const page = await mountPage(backend);
    await settle();
    expect(backend.muxReads).toBe(2);
    await hydrate(backend);
    await settleUntil(() => backend.muxReads === 3);
    await act(async () => backend.takeRead().reject(new Error(SNAPSHOT_BUSY)));
    await act(async () => replacedRead.resolve(snapshot(11, { withGeneral: false })));
    await settle();
    expect(page.state().workspaces).toEqual([]);

    backend.emit("mux-state-changed");
    await settleUntil(() => backend.muxReads === 4);
    await act(async () => backend.takeRead().resolve(snapshot(12)));
    await settle();
    expectGeneralSurvives(page.state());
    expect(page.state().generation).toBe(12);
  });

  it("rejects stale generations that would drop General after a newer response", async () => {
    const backend = createBackend();
    const page = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve(snapshot(20)));
    await settle();
    expectGeneralSurvives(page.state());

    backend.emit("mux-state-changed", snapshot(19, { withGeneral: false }));
    await settle();
    expectGeneralSurvives(page.state());

    backend.emit("mux-state-changed");
    await settleUntil(() => backend.muxReads === 2);
    await act(async () => backend.takeRead().resolve(snapshot(18, { withGeneral: false })));
    await settle();
    expectGeneralSurvives(page.state());
    expect(page.state().generation).toBe(20);

    backend.emit("mux-state-changed", snapshot(21, { mainTitle: "Main" }));
    await settle();
    expectGeneralSurvives(page.state());
    expect(page.state().generation).toBe(21);
  });
});

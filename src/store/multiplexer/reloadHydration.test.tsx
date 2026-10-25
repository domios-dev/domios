import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MultiplexerState, Tab, Workspace } from "../../lib/types";
import type { ExtendedState } from "./tree";
import * as bridge from "../../lib/bridge";
import * as store from "./index";
import { partitionSidebarWorkspaces } from "../../components/Sidebar/sidebarSections";
import { tabsInWorkspace } from "../../components/Sidebar/workspaceTabs";
import { sortByOrder } from "../../components/Sidebar/sortable";
import { isGeneralAgent } from "../../lib/roleKind";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
vi.hoisted(() => {
  (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
});

const SNAPSHOT_BUSY = "state snapshot unavailable while tab state is initializing or changing";
const GENERAL_ID = "bfb90821";
const MAIN_ID = "46137477";
const ORDINARY_ID = "ws-ordinary";
const ORDINARY_TAB_ID = "tab-ordinary";

type Store = typeof import("./index");
type Handler = (event: { payload: unknown }) => void;

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
function createBackend(commands: Record<string, () => unknown> = {}) {
  const reads: Array<Deferred<MultiplexerState>> = [];
  const binds: Array<Deferred<void>> = [];
  const listeners = new Map<string, Set<Handler>>();
  let muxReads = 0;
  return {
    invoke: async <T,>(cmd: string): Promise<T> => {
      if (commands[cmd]) return await commands[cmd]() as T;
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
  const holder: { state: ExtendedState | null; actions: ReturnType<Store["useMultiplexer"]> | null } = { state: null, actions: null };
  function Probe() {
    holder.state = store.useMuxState() as ExtendedState;
    holder.actions = store.useMultiplexer();
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
    actions: () => {
      if (!holder.actions) throw new Error("mux actions not rendered");
      return holder.actions;
    },
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
  it("refetches when an off-chain command snapshot makes the reducer decline a stream delta", async () => {
    const backend = createBackend({ spawn_general_agent_session: () => ({
      snapshot: snapshot(100), tab_id: MAIN_ID, workspace_id: GENERAL_ID,
    }) });
    const page = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve({ ...snapshot(10), tab_statuses: { [MAIN_ID]: "Working" } }));
    await act(async () => { await page.actions().spawnAgentTab(GENERAL_ID, "fixture", "/general", undefined, undefined, undefined, "fresh_general"); });
    expect(page.state().generation).toBe(100);
    await act(async () => backend.emit("mux-state-changed", {
      base_generation: 10, generation: 11,
      status_deltas: { [MAIN_ID]: { status: "Idle", agent_state: { state: "idle" } } },
    }));
    expect(page.state().muxResyncRequired).toBe(true);
    expect(backend.muxReads).toBe(2);
    await act(async () => backend.takeRead().resolve({ ...snapshot(11), tab_statuses: { [MAIN_ID]: "Idle" } }));
    expect(page.state().generation).toBe(11);
    expect(page.state().muxResyncRequired).toBe(false);
    await act(async () => backend.emit("mux-state-changed", {
      base_generation: 11, generation: 12,
      status_deltas: { [MAIN_ID]: { status: null, agent_state: null } },
    }));
    expect(page.state().generation).toBe(12);
    expect(page.state().tabStatuses[MAIN_ID]).toBeUndefined();
    expect(page.state().tabs[MAIN_ID].agent_state).toBeNull();
    expect(backend.muxReads).toBe(2);
  });

  it("restores a rejected close from an unchanged published generation", async () => {
    const close = deferred<void>();
    const backend = createBackend({ mux_close_tab: () => close.promise });
    const page = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve(snapshot(10)));
    await act(async () => page.actions().closeTab(ORDINARY_ID, ORDINARY_TAB_ID));
    expect(page.state().tabs[ORDINARY_TAB_ID]).toBeUndefined();
    await act(async () => close.reject(new Error("fixture close refusal")));
    expect(backend.muxReads).toBe(2);
    await act(async () => backend.takeRead().resolve(snapshot(10)));
    expect(page.state().generation).toBe(10);
    expect(page.state().tabs[ORDINARY_TAB_ID]).toBeDefined();
    expect(page.state().workspaces.find((workspace) => workspace.id === ORDINARY_ID)?.tabs).toEqual([ORDINARY_TAB_ID]);
    expect(page.state().pendingCloses).toEqual([]);
  });

  it.each(["reorder", "move"])("restores a rejected %s without a structural generation change", async (operation) => {
    const mutation = deferred<void>();
    const backend = createBackend({ [operation === "move" ? "mux_move_tab" : "mux_reorder_workspace"]: () => mutation.promise });
    const page = await mountPage(backend);
    await hydrate(backend);
    const current = snapshot(10);
    current.tabs.spare = { id: "spare", title: "Spare" } as Tab;
    current.workspaces.find((workspace) => workspace.id === ORDINARY_ID)!.tabs.push("spare");
    current.workspaces.push({ id: "target", name: "Target", tabs: [], active_tab_id: null, cwd: null, sort_order: 2 });
    await act(async () => backend.takeRead().resolve(current));
    await act(async () => {
      if (operation === "move") page.actions().moveTab(ORDINARY_TAB_ID, ORDINARY_ID, "target");
      else page.actions().reorderWorkspaces([GENERAL_ID, "target", ORDINARY_ID]);
    });
    if (operation === "move") expect(page.state().workspaces.find((workspace) => workspace.id === "target")?.tabs).toContain(ORDINARY_TAB_ID);
    else expect(page.state().workspaces.find((workspace) => workspace.id === ORDINARY_ID)?.sort_order).toBe(2);
    await act(async () => mutation.reject(new Error("fixture mutation refusal")));
    expect(backend.muxReads).toBe(2);
    await act(async () => backend.takeRead().resolve(structuredClone(current)));
    expect(page.state().generation).toBe(10);
    expect(page.state().workspaces.find((workspace) => workspace.id === ORDINARY_ID)?.sort_order).toBe(1);
    expect(page.state().workspaces.find((workspace) => workspace.id === ORDINARY_ID)?.tabs).toEqual([ORDINARY_TAB_ID, "spare"]);
    expect(page.state().workspaces.find((workspace) => workspace.id === "target")?.tabs).toEqual([]);
  });

  it("does not let an older forced rollback read overwrite a newer status delta", async () => {
    const close = deferred<void>();
    const backend = createBackend({ mux_close_tab: () => close.promise });
    const page = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve(snapshot(10)));
    await act(async () => page.actions().closeTab(ORDINARY_ID, ORDINARY_TAB_ID));
    await act(async () => close.reject(new Error("fixture close refusal")));
    const rollbackRead = backend.takeRead();
    await act(async () => backend.emit("mux-state-changed", {
      base_generation: 10, generation: 12,
      status_deltas: { [MAIN_ID]: { status: "Idle", agent_state: { state: "idle" } } },
    }));
    await act(async () => rollbackRead.resolve(snapshot(10)));
    expect(page.state().generation).toBe(12);
    expect(page.state().tabStatuses[MAIN_ID]).toBe("Idle");
    expect(backend.muxReads).toBe(3);
    await act(async () => backend.takeRead().resolve({ ...snapshot(12), tab_statuses: { [MAIN_ID]: "Idle" } }));
    expect(page.state().tabs[ORDINARY_TAB_ID]).toBeDefined();
    expect(page.state().generation).toBe(12);
  });

  it("applies a status burst without reads, refetches once for a gap, and resumes deltas", async () => {
    const backend = createBackend();
    const page = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve(snapshot(10)));
    await settleUntil(() => page.state().generation === 10);
    const workspaces = page.state().workspaces;
    await act(async () => {
      for (let i = 0; i < 126; i++) backend.emit("mux-state-changed", {
        base_generation: 10 + i * 3, generation: 13 + i * 3,
        status_deltas: { [MAIN_ID]: { status: i % 2 ? "Idle" : "Working", agent_state: { state: i % 2 ? "idle" : "working" } } },
      });
    });
    expect(page.state().generation).toBe(388);
    expect(page.state().tabStatuses[MAIN_ID]).toBe("Idle");
    expect(page.state().tabs[MAIN_ID].agent_state).toEqual({ state: "idle" });
    expect(page.state().workspaces).toBe(workspaces);
    expect(backend.muxReads).toBe(1);

    await act(async () => {
      backend.emit("mux-state-changed", { base_generation: 391, generation: 394, status_deltas: {} });
      backend.emit("mux-state-changed", { base_generation: 394, generation: 397, status_deltas: {} });
    });
    expect(backend.muxReads).toBe(2);
    await act(async () => backend.takeRead().resolve(snapshot(400)));
    await settleUntil(() => page.state().generation === 400);
    expect(backend.muxReads).toBe(2);
    await act(async () => backend.emit("mux-state-changed", {
      base_generation: 400, generation: 403,
      status_deltas: { [MAIN_ID]: { status: "Working", agent_state: { state: "working" } } },
    }));
    expect(page.state().generation).toBe(403);
    expect(page.state().tabStatuses[MAIN_ID]).toBe("Working");
    expect(backend.muxReads).toBe(2);
  });

  it("keeps newer deltas when an older read completes and resets the base after a daemon restart", async () => {
    const backend = createBackend();
    const page = await mountPage(backend);
    await hydrate(backend);
    await act(async () => backend.takeRead().resolve(snapshot(10)));
    await settleUntil(() => page.state().generation === 10);
    await act(async () => backend.emit("mux-state-changed"));
    const oldRead = backend.takeRead();
    await act(async () => backend.emit("mux-state-changed", {
      base_generation: 10, generation: 15,
      status_deltas: { [MAIN_ID]: { status: "Idle", agent_state: { state: "idle" } } },
    }));
    await act(async () => oldRead.resolve(snapshot(12)));
    expect(page.state().generation).toBe(15);
    expect(page.state().tabStatuses[MAIN_ID]).toBe("Idle");
    await act(async () => window.dispatchEvent(new Event("codeterm-web-boot-changed")));
    await act(async () => backend.emit("mux-state-changed", {
      base_generation: 1, generation: 2, status_deltas: {},
    }));
    expect(backend.muxReads).toBe(3);
    await act(async () => backend.takeRead().resolve(snapshot(3)));
    expect(page.state().generation).toBe(3);
  });

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

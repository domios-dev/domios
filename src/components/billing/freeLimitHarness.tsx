import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { MultiplexerState, Tab, Team, Workspace } from "../../lib/types";
import { FreeLimitPlansModal } from "./FreeLimitPlansModal";
import { freeLimitGate } from "./freeLimitGate";
import { FREE_LIMIT_REACHED, type FreeLimits, type LocalLimitKey } from "./freeLimitModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

type Store = typeof import("../../store/multiplexer");
type Actions = ReturnType<Store["useMultiplexer"]>;

export const WORKSPACE = "ws-main";
const GENERAL = "ws-general";

const daemonRefusal = (key: LocalLimitKey) => `${FREE_LIMIT_REACHED}: opaque-refusal-body (limit: ${key})`;

/** Seeds a daemon snapshot: the protected General workspace plus `workspaces` ordinary ones, `teams`, `tabs`. */
export function seedState({ workspaces = 1, teams = 0, tabs = 0 }: { workspaces?: number; teams?: number; tabs?: number }): MultiplexerState {
  const tabRecords: Record<string, Tab> = {};
  const tabIds = Array.from({ length: tabs }, (_, index) => `tab-${index}`);
  for (const id of tabIds) tabRecords[id] = { id, pty_id: null, title: id, cwd: null } as unknown as Tab;
  const ordinary = Array.from({ length: workspaces }, (_, index): Workspace => ({
    id: index === 0 ? WORKSPACE : `ws-${index}`, name: `Workspace ${index}`, tabs: index === 0 ? tabIds : [],
    active_tab_id: null, cwd: `/repo/${index}`, protected: false,
  } as unknown as Workspace));
  return {
    workspaces: [{ id: GENERAL, name: "General", tabs: [], active_tab_id: null, cwd: null, protected: true } as unknown as Workspace, ...ordinary],
    tabs: tabRecords,
    teams: Array.from({ length: teams }, (_, index) => ({ id: `team-${index}`, lifecycle: "active", created_at: 0, workspace_id: WORKSPACE }) as Team),
    active_workspace_id: WORKSPACE,
    focused_tab_id: null,
    generation: 1,
  };
}

/** A scripted daemon that admits creation with the same rule as the entitlements gate and keeps every existing item. */
export function fakeDaemon(state: MultiplexerState, enforced: FreeLimits | null) {
  const calls: string[] = [];
  const failures = new Map<string, string>();
  let next = 0;
  const snapshot = () => ({ ...state, generation: ++state.generation! });
  const counts = () => ({
    workspaces: state.workspaces.filter((workspace) => !workspace.protected).length,
    teams: state.teams?.length ?? 0,
    "agent.tabs": Object.keys(state.tabs).length,
  });
  const admit = (key: LocalLimitKey, extra = 0) => {
    const max = enforced?.enforcing ? enforced.max[key] : null;
    if (max !== null && max !== undefined && counts()[key] + extra >= max) throw daemonRefusal(key);
  };
  const addTab = () => {
    const id = `new-tab-${++next}`;
    state.tabs = { ...state.tabs, [id]: { id, pty_id: null, title: id, cwd: null } as unknown as Tab };
    state.workspaces = state.workspaces.map((workspace) => workspace.id === WORKSPACE ? { ...workspace, tabs: [...workspace.tabs, id] } : workspace);
    return id;
  };
  const addTeam = () => { state.teams = [...(state.teams ?? []), { id: `new-team-${++next}`, lifecycle: "active", created_at: 0, workspace_id: WORKSPACE } as Team]; };
  const invoke = async <T,>(command: string): Promise<T> => {
    calls.push(command);
    const failure = failures.get(command);
    if (failure) throw failure;
    switch (command) {
      case "mux_get_state": return snapshot() as T;
      case "get_mesh_peers": return { peers: [] } as T;
      case "get_config": return { api: { enabled: false }, agent: {} } as T;
      case "ordinary_tabs_list": case "list_notifications": case "list_agent_providers": return [] as T;
      case "mux_create_tab": admit("agent.tabs"); return { id: addTab(), title: "shell" } as T;
      case "mux_create_workspace": {
        admit("workspaces");
        const id = `new-ws-${++next}`;
        state.workspaces = [...state.workspaces, { id, name: id, tabs: [], active_tab_id: null, cwd: `/new/${id}` } as unknown as Workspace];
        return { id, name: id } as T;
      }
      case "mux_spawn_tab": case "spawn_general_agent_session": case "fork_agent_tab":
        admit("agent.tabs");
        return { workspace_id: WORKSPACE, tab_id: addTab(), snapshot: snapshot() } as T;
      case "mux_create_orchestration_pair": {
        admit("agent.tabs", 1);
        admit("teams");
        addTeam();
        return { orchestrator_tab_id: addTab(), planner_tab_id: addTab(), team_id: "new-team" } as T;
      }
      case "mux_make_orchestrator": admit("teams"); addTeam(); return undefined as T;
      default: return undefined as T;
    }
  };
  return {
    invoke,
    calls,
    created: (command: string) => calls.filter((call) => call === command).length,
    failWith: (command: string, error: string) => failures.set(command, error),
    counts,
  };
}

export type Daemon = ReturnType<typeof fakeDaemon>;

const mounted: Root[] = [];

async function settleUntil(condition: () => boolean, turns = 200): Promise<void> {
  for (let i = 0; i < turns; i++) {
    if (condition()) return;
    await act(async () => { await Promise.resolve(); });
  }
  throw new Error(`condition not met within ${turns} microtask turns`);
}

/** The real store and the one plans modal against `daemon`; `upgrade` stands in for the checkout call. */
export async function mountApp(daemon: Daemon, upgrade: (planKey: string) => Promise<void> = async () => {}) {
  const bridge = await import("../../lib/bridge");
  const store = await import("../../store/multiplexer");
  bridge.__setInvokeForTest(daemon.invoke as typeof bridge.invoke);
  bridge.__setListenForTest(async () => () => {});
  const holder: { actions?: Actions; state?: ReturnType<Store["useMuxState"]> } = {};
  function Probe() {
    holder.actions = store.useMultiplexer();
    holder.state = store.useMuxState();
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push(root);
  await act(async () => {
    root.render(createElement(store.MultiplexerProvider, null, createElement(Probe), createElement(FreeLimitPlansModal, { upgrade })));
  });
  await settleUntil(() => (holder.state?.workspaces.length ?? 0) > 0);
  return {
    actions: () => holder.actions!,
    state: () => holder.state!,
    run: async <T,>(action: () => Promise<T>): Promise<T> => {
      let result!: T;
      await act(async () => { result = await action(); });
      await act(async () => { await Promise.resolve(); });
      return result;
    },
  };
}

export const modal = () => document.querySelector<HTMLElement>("[role=dialog]");
export const modalLine = () => modal()?.querySelector("h2")?.textContent ?? null;
export const toastTitles = (state: { toasts?: Array<{ title: string }> }) => (state.toasts ?? []).map((toast) => toast.title);

export async function unmountAll() {
  for (const root of mounted.splice(0)) await act(async () => root.unmount());
  document.body.replaceChildren();
  act(() => freeLimitGate.close());
  freeLimitGate.setLimits(null);
  const bridge = await import("../../lib/bridge");
  bridge.__setInvokeForTest(null);
  bridge.__setListenForTest(null);
}

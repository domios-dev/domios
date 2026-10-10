import assert from "node:assert/strict";
import type { AtomicSpawn, MultiplexerState, Tab } from "../../lib/types";
import { setupDom } from "./SidebarWorkspaceTabs.testkit";

process.env.VITE_CODETERM_TEST = "1";
setupDom();
Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
Object.defineProperty(globalThis, "sessionStorage", { value: window.sessionStorage, configurable: true });
(globalThis as { __BUILD_HASH__?: string }).__BUILD_HASH__ = "general-session-test";

async function run() {
  const { act } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const { MultiplexerProvider, useMuxState } = await import("../../store/multiplexer");
  const { GeneralWorkspaceTabs } = await import("./GeneralWorkspaceTabs");
  const { SidebarDnd } = await import("./sortable");
  const main: Tab = {
    id: "main-tab", title: "Main", protected: true, role: "Manager", role_profile: "general_agent",
    cwd: "/instance/general", pty_id: "main-pty", tab_type: "Terminal",
    agent_type: "configured-provider", agent_session_id: "main-session", session_binding_nonce: "main-nonce",
  };
  let snapshot: MultiplexerState = {
    workspaces: [{ id: "general", name: "General", protected: true, cwd: "/instance/general", tabs: [main.id], active_tab_id: main.id }],
    tabs: { [main.id]: main }, active_workspace_id: "general", focused_tab_id: main.id, generation: 1,
  };
  let observed: MultiplexerState = snapshot;
  const requests: Array<{ command: string; args?: Record<string, unknown> }> = [];
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    requests.push({ command, args });
    if (command === "mux_get_state") return snapshot as T;
    if (command === "spawn_general_agent_session") {
      const fresh: Tab = {
        id: "fresh-tab", title: "Fresh", role: "Manager", role_profile: "general_agent",
        cwd: "/instance/general", pty_id: "fresh-pty", tab_type: "Terminal",
        agent_type: "configured-provider", agent_session_id: "fresh-session",
      };
      snapshot = {
        ...snapshot, generation: 2,
        workspaces: [{ ...snapshot.workspaces[0], tabs: [main.id, fresh.id] }],
        tabs: { ...snapshot.tabs, [fresh.id]: fresh },
      };
      const result: AtomicSpawn = { snapshot, tab_id: fresh.id, workspace_id: "general" };
      return result as unknown as T;
    }
    return [] as unknown as T;
  });
  function Probe() {
    const state = useMuxState();
    observed = state;
    const workspace = state.workspaces[0];
    if (!workspace) return null;
    return <SidebarDnd handlers={{ reorderTabs: () => {}, reorderDisplay: () => {}, reorderWorkspaces: () => {} }}>
      <GeneralWorkspaceTabs
        workspace={workspace} providerId="configured-provider" generalCwd={workspace.cwd}
        activeTabId={workspace.active_tab_id} sortedTabs={workspace.tabs.map((id) => state.tabs[id])}
        tabs={state.tabs} tabStatuses={state.tabStatuses} teams={[]} planIndex={[]} plans={[]}
        activeWork={null} providers={[]} closingTabIds={new Set()} appearingTabIds={new Set()}
        isActiveVisual={true} workspaceCanvasActive={false} touchMode={true} compact={false}
        sidebarActionText="" orchestrators={[]} onBeforeOpenPlan={() => {}}
        onActivateTab={() => {}} onCloseTab={() => {}} onRenameTab={() => {}}
      />
    </SidebarDnd>;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MultiplexerProvider><Probe /></MultiplexerProvider>));
    const button = Array.from(host.querySelectorAll("button")).find((node) => node.textContent?.trim() === "New session");
    assert(button && !button.disabled, "General exposes an enabled New session button");
    await act(async () => button.click());
    const spawns = requests.filter((request) => request.command === "spawn_general_agent_session");
    assert.deepEqual(spawns, [{ command: "spawn_general_agent_session", args: { params: {
      workspaceId: "general", providerId: "configured-provider", cwd: "/instance/general",
      role: "general_agent", intent: "fresh_general",
    } } }]);
    assert(!requests.some((request) => request.command === "mux_spawn_tab"));
    for (const key of ["id", "title", "protected", "role", "role_profile", "cwd", "pty_id", "agent_type", "agent_session_id", "session_binding_nonce"] as const) {
      assert.equal(observed.tabs[main.id][key], main[key], `Main ${key} stays unchanged`);
    }
    assert.equal(observed.focused_tab_id, main.id);
    assert.equal(observed.workspaces[0].active_tab_id, main.id);
    assert(host.querySelector('[data-tab-id="fresh-tab"]'), "the returned independent session is visible");
    assert(host.querySelector('[data-tab-id="main-tab"] button[aria-current="page"]'), "Main stays selected");
    console.log("PASS General New session: one General request, fresh visible row, Main unchanged");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}

run().catch((error) => { console.error(error); process.exitCode = 1; });

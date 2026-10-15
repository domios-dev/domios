import "../Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { nativeBridge } from "../../lib/nativeBridge";
import { TransportError } from "../../lib/transportClient";
import { MultiplexerProvider, useMultiplexer, useMuxState } from "../../store/multiplexer";
import { TabBootOverlay } from "../Terminal/bootOverlay";
import type { MultiplexerActionsContextType } from "../../store/multiplexer/context";

test("choosing a replacement folder clears the workspace error through the store snapshot", async () => {
  let cwd = "/retired/project";
  let missing = true;
  let generation = 1;
  const changes: unknown[] = [];
  let actions!: MultiplexerActionsContextType;
  let notices = -1;
  const refusal = new TransportError(422, "/api/agents/spawn", JSON.stringify({
    error: { code: "workspace_cwd_missing", message: "opaque backend refusal" },
  }));
  const snapshot = () => ({
    workspaces: [{ id: "existing", name: "Project", tabs: [], active_tab_id: null, cwd, cwd_missing: missing }],
    tabs: {}, teams: [], active_workspace_id: "existing", focused_tab_id: null, generation: generation++,
  });
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "mux_get_state") return snapshot() as T;
    if (command === "mux_set_workspace_cwd") {
      changes.push(args);
      cwd = String(args?.cwd);
      missing = false;
      return { workspace_id: "existing", cwd, warnings: [] } as T;
    }
    if (command === "mux_spawn_tab") throw refusal;
    if (command === "get_config") return { api: { enabled: false }, agent: {} } as T;
    if (command === "get_mesh_peers") return { peers: [] } as T;
    if (["list_notifications", "ordinary_tabs_list", "list_agent_providers"].includes(command)) return [] as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  assert(nativeBridge.dialogs);
  const originalOpen = nativeBridge.dialogs.open;
  nativeBridge.dialogs.open = async () => "/replacement/project";
  function View() {
    actions = useMultiplexer();
    const state = useMuxState();
    notices = state.notifications.length + state.toasts.length;
    const workspace = state.workspaces.find((item) => item.id === "existing");
    return workspace?.cwd_missing ? <TabBootOverlay workspace={workspace} cwd={cwd} shellReady={false} overlayVisible view={{ kind: "failed", reason: "Failed to spawn shell: CreateProcessW failed (os error 267)", canRetry: false, canStartFreshAgent: false, canStartNewSession: false, canKeepShell: false }} onRetry={() => {}} onStartFreshAgent={() => {}} onStartNewSession={() => {}} onKeepShell={() => {}} />
      : <p role="status">{workspace?.cwd}</p>;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MultiplexerProvider><View /></MultiplexerProvider>));
    const choose = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Choose folder");
    assert(choose, "the workspace offers an accessible folder chooser");
    assert(host.querySelector('[role="alert"]'));
    await act(async () => choose.click());
    assert.deepEqual(changes, [{ workspaceId: "existing", cwd: "/replacement/project" }]);
    assert.equal(host.querySelector('[role="alert"]'), null);
    assert.equal(host.querySelector('[role="status"]')?.textContent, "/replacement/project");
    await act(async () => {
      await assert.rejects(actions.spawnAgentTab("existing", "fixture"), (error) => error === refusal);
    });
    assert.equal(notices, 0, "store errors return to the caller without a toast or bell notification");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    nativeBridge.dialogs.open = originalOpen;
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
});

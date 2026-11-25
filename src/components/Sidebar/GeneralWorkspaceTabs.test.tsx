import assert from "node:assert/strict";
import { describe, it, vi } from "vitest";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { MultiplexerProvider, useMuxState } from "../../store/multiplexer";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { setSidebarMachineFilter, useSidebarMachineFilter } from "./sidebarMachineFilter";
import { GeneralWorkspaceTabs } from "./GeneralWorkspaceTabs";
import { NewSessionPicker } from "../Terminal/NewSessionPicker";
import { SidebarDnd } from "./sortable";

const fixture = vi.hoisted(() => ({ providers: [
  { id: "configured-provider", display_name: "Configured", available_models: [{ id: "default", display_name: "Default" }], default_model: "default" },
  { id: "chosen-provider", display_name: "Chosen provider", available_models: [{ id: "first", display_name: "First" }, { id: "chosen-model", display_name: "Chosen model" }], default_model: "first",
    reasoning_efforts: [{ id: "high", display_name: "High" }] },
], loading: false }));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => fixture }));
const presetFixture = vi.hoisted(() => ({ presets: [], loading: false, error: null }));
vi.mock("../../hooks/useModelPresets", () => ({ useModelPresets: () => presetFixture }));
vi.mock("../../lib/providerInstallStatus", () => ({ probeProviderInstallMap: async () => ({}) }));
import type { AtomicSpawn, MultiplexerState, Tab } from "../../lib/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function run(paired: Array<Record<string, unknown>> = []) {
  const main: Tab = {
    id: "main-tab", title: "Main", protected: true, role: "Manager", role_profile: "general_agent",
    cwd: "/instance/general", pty_id: "main-pty", tab_type: "Terminal",
    agent_type: "configured-provider", agent_session_id: "main-session", session_binding_nonce: "main-nonce",
  };
  let snapshot: MultiplexerState = {
    workspaces: [{ id: "general", name: "General", protected: true, cwd: "/instance", tabs: [main.id], active_tab_id: main.id }],
    tabs: { [main.id]: main }, active_workspace_id: "general", focused_tab_id: main.id, generation: 1,
  };
  let observed: MultiplexerState = snapshot;
  const requests: Array<{ command: string; args?: Record<string, unknown> }> = [];
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    requests.push({ command, args });
    if (command === "mux_get_state") return snapshot as T;
    if (command === "mesh_list_peers") return { local_instance_name: "LOCAL", paired } as T;
    if (command === "spawn_general_agent_session") {
      if (args?.peerId === "machine-x") return { snapshot, tab_id: "remote-fresh-tab", workspace_id: "peer-general" } as T;
      const fresh: Tab = {
        id: "fresh-tab", title: "Fresh", role: "Manager", role_profile: "general_agent",
        cwd: "/instance/general", pty_id: "fresh-pty", tab_type: "Terminal",
        agent_type: "chosen-provider", agent_session_id: "fresh-session",
      };
      snapshot = {
        ...snapshot, generation: 2,
        workspaces: [{ ...snapshot.workspaces[0], tabs: [main.id, fresh.id] }],
        tabs: { ...snapshot.tabs, [fresh.id]: fresh },
      };
      const result: AtomicSpawn = { snapshot, tab_id: fresh.id, workspace_id: "general" };
      return result as unknown as T;
    }
    if (command === "mesh_peer_tabs_list") return {
      peer_id: "machine-x", peer_name: "BLACKSTAR", tabs: [{
        id: "remote-fresh-tab", title: "Fresh", role: "Manager", role_profile: "general_agent",
        cwd: "/peer/general", pty_id: "remote-pty", tab_type: "Terminal", agent_type: "peer-provider",
      }],
    } as T;
    return [] as unknown as T;
  });
  function Probe() {
    const [picker, setPicker] = useState<OpenNewSessionDetail | null>(null);
    useEffect(() => {
      const open = (event: Event) => setPicker((event as CustomEvent<OpenNewSessionDetail>).detail);
      window.addEventListener(OPEN_NEW_SESSION_EVENT, open);
      return () => window.removeEventListener(OPEN_NEW_SESSION_EVENT, open);
    }, []);
    const state = useMuxState();
    const machineFilter = useSidebarMachineFilter();
    observed = state;
    const workspace = state.workspaces[0];
    if (!workspace) return null;
    return <>
    <span data-testid="machine-filter" data-value={machineFilter === "all" ? "all" : machineFilter === "local" ? "local" : machineFilter.peerId}
      data-focused-tab={state.focused_tab_id ?? ""} />
    <SidebarDnd handlers={{ reorderTabs: () => {}, reorderDisplay: () => {}, reorderWorkspaces: () => {} }}>
      <GeneralWorkspaceTabs
        workspace={workspace} providerId="configured-provider" generalCwd={workspace.cwd}
        activeTabId={workspace.active_tab_id} sortedTabs={workspace.tabs.map((id) => state.tabs[id])}
        tabs={state.tabs} tabStatuses={state.tabStatuses} teams={[]} planIndex={[]} plans={[]}
        providers={[]} closingTabIds={new Set()} appearingTabIds={new Set()}
        isActiveVisual={true} workspaceCanvasActive={false} touchMode={true} compact={false}
        sidebarActionText="" orchestrators={[]} onBeforeOpenPlan={() => {}}
        onActivateTab={() => {}} onCloseTab={() => {}} onRenameTab={() => {}}
      />
      {picker && <NewSessionPicker {...picker} onClose={() => setPicker(null)} />}
    </SidebarDnd>
    </>;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    if (paired.length > 0) setSidebarMachineFilter({ id: "filtered-peer", name: "Filtered peer" });
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "pair", orchestratorProviderId: "configured-provider", plannerProviderId: "configured-provider" }));
    await act(async () => root.render(<MultiplexerProvider><Probe /></MultiplexerProvider>));
    const machineButton = host.querySelector<HTMLButtonElement>('[aria-label="New session on another machine"]');
    if (paired.length > 0) {
      assert(machineButton, "paired machines expose the existing General monitor button");
      await act(async () => machineButton.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
      const local = document.body.querySelector<HTMLElement>('[data-testid="new-general-session-machine-option-__local_machine__"]');
      assert.equal(local?.getAttribute("aria-checked"), "true", "the current machine receives the selected-row state");
      const offline = document.body.querySelector<HTMLElement>('[data-testid="new-general-session-machine-option-offline"]');
      assert.equal(offline?.getAttribute("aria-disabled"), "true", "offline machines cannot be chosen");
      const reasonId = offline?.getAttribute("aria-describedby");
      assert(reasonId && document.getElementById(reasonId)?.textContent, "the unavailable machine exposes its reason");
      const remote = document.body.querySelector<HTMLElement>('[data-testid="new-general-session-machine-option-machine-x"]');
      assert(remote?.textContent?.includes("BLACKSTAR"));
      assert(remote?.textContent?.includes("online"));
      await act(async () => remote!.click());
      const remoteSpawn = requests.find((request) => request.command === "spawn_general_agent_session" && request.args?.peerId === "machine-x");
      assert.deepEqual(remoteSpawn?.args, { peerId: "machine-x", params: { role: "general_agent", intent: "fresh_general" } });
      const sidebarState = host.querySelector('[data-testid="machine-filter"]');
      assert.equal(sidebarState?.getAttribute("data-value"), "all");
      assert.equal(sidebarState?.getAttribute("data-focused-tab"), "mesh:machine-x/remote-fresh-tab");
      return;
    }
    assert.equal(machineButton, null, "the monitor action stays hidden with one machine");
    const button = Array.from(host.querySelectorAll("button")).find((node) => node.textContent?.trim() === "New session");
    assert(button && !button.disabled, "General exposes an enabled New session button");
    await act(async () => button.click());
    assert.equal(requests.filter((request) => request.command === "spawn_general_agent_session").length, 0);
    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]');
    assert(dialog, "General New session opens the shared modal");
    assert(!dialog.querySelector('button[aria-label="Team"]'));
    assert(!dialog.querySelector('button[aria-label="Orchestrator provider"]'));
    async function choose(label: string, value: string) {
      const control = Array.from(dialog!.querySelectorAll<HTMLButtonElement>("button[aria-label]"))
        .find((button) => button.getAttribute("aria-label") === label || button.getAttribute("aria-label")?.startsWith(`${label}:`));
      assert(control);
      await act(async () => { control.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })); });
      const item = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]')).find((node) => node.textContent?.trim() === value);
      assert(item);
      await act(async () => item.click());
    }
    await choose("Agent provider", "Chosen provider");
    await choose("Agent model", "Chosen model");
    assert(Array.from(dialog.querySelectorAll<HTMLButtonElement>("button[aria-label]"))
      .some((button) => button.getAttribute("aria-label")?.startsWith("Agent effort:")));
    const submit = Array.from(dialog.querySelectorAll("button")).find((node) => node.textContent?.trim() === "Start session");
    assert(submit);
    await act(async () => submit.click());
    await act(async () => document.querySelector('[data-testid="new-session-picker"]')?.dispatchEvent(new Event("animationend", { bubbles: true })));
    const spawns = requests.filter((request) => request.command === "spawn_general_agent_session");
    assert.deepEqual(spawns, [{ command: "spawn_general_agent_session", args: { params: {
      workspaceId: "general", providerId: "chosen-provider",
      model: "chosen-model", reasoningEffort: "high", preset: undefined, accountId: undefined,
      role: "general_agent", intent: "fresh_general",
    } } }]);
    assert(!requests.some((request) => request.command === "mux_spawn_tab"));
    for (const key of ["id", "title", "protected", "role", "role_profile", "cwd", "pty_id", "agent_type", "agent_session_id", "session_binding_nonce"] as const) {
      assert.equal(observed.tabs[main.id][key], main[key], `Main ${key} stays unchanged`);
    }
    assert(host.querySelector('[data-tab-id="fresh-tab"]'), "the returned independent session is visible");
    assert(!document.body.querySelector('[role="dialog"]'), "successful spawn closes the modal");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
    setSidebarMachineFilter("all");
  }
}

describe("General New session", () => {
  it("opens the agent-only modal and spawns the chosen settings once into General", () => run());
  it("uses the existing monitor action to create with peer defaults on the selected machine", () => run([
    { instance_id: "machine-x", instance_name: "BLACKSTAR", public_key: "blackstar-key", online: true, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION },
    { instance_id: "offline", instance_name: "OFFLINE", public_key: "offline-key", online: false, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION },
  ]));
});

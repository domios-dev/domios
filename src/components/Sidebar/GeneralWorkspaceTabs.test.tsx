import assert from "node:assert/strict";
import { describe, it, vi } from "vitest";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { MultiplexerProvider, useMuxState } from "../../store/multiplexer";
import { PairedWorkspaceInventoryContext } from "../../store/multiplexer/context";
import type { GeneralAvailability, PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";
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
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function mountGeneral(generals: PairedGeneral[] = [], refreshPeerInventory = vi.fn()) {
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
    if (command === "mesh_list_peers") return {
      local_instance_name: "LOCAL",
      paired: generals.map((peer) => ({
        instance_id: peer.peerId,
        instance_name: peer.name,
        public_key: `${peer.peerId}-key`,
        online: peer.availability === "online",
        access_level: "full",
        relay_protocol_version: peer.availability === "update-required" ? RELAY_PROTOCOL_VERSION - 1 : RELAY_PROTOCOL_VERSION,
      })),
    } as T;
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
      <PairedWorkspaceInventoryContext.Provider value={{ generals, browse: null, refreshPeerInventory }}>
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
      </PairedWorkspaceInventoryContext.Provider>
    </>;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<MultiplexerProvider><Probe /></MultiplexerProvider>));
  return {
    host,
    main,
    requests,
    refreshPeerInventory,
    observed: () => observed,
    close: async () => {
      await act(async () => root.unmount());
      host.remove();
      __setInvokeForTest(null);
      __setListenForTest(null);
      setSidebarMachineFilter("all");
    },
  };
}

describe("General New session", () => {
  it("opens the agent-only modal without a remote machine picker and spawns the chosen settings once into General", async () => {
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "pair", orchestratorProviderId: "configured-provider", plannerProviderId: "configured-provider" }));
    const view = await mountGeneral([
      { peerId: "machine-x", name: "BLACKSTAR", availability: "online", sessions: [] },
    ]);
    try {
      assert.equal(view.host.querySelector('[aria-label="New session on another machine"]'), null);
      const button = view.host.querySelector<HTMLButtonElement>('[data-testid="sidebar-new-session"]');
      assert(button && !button.disabled, "General exposes an enabled New session button");
      await act(async () => button.click());
      assert.equal(view.requests.filter((request) => request.command === "spawn_general_agent_session").length, 0);
      const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]');
      assert(dialog, "General New session opens the shared modal");
      assert(!dialog.querySelector('button[aria-label="Team"]'));
      assert(!dialog.querySelector('button[aria-label="Orchestrator provider"]'));
      async function choose(label: string, value: string) {
        const control = Array.from(dialog!.querySelectorAll<HTMLButtonElement>("button[aria-label]"))
          .find((node) => node.getAttribute("aria-label") === label || node.getAttribute("aria-label")?.startsWith(`${label}:`));
        assert(control);
        await act(async () => { control.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })); });
        const item = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]')).find((node) => node.textContent?.trim() === value);
        assert(item);
        await act(async () => item.click());
      }
      await choose("Agent provider", "Chosen provider");
      await choose("Agent model", "Chosen model");
      assert(Array.from(dialog.querySelectorAll<HTMLButtonElement>("button[aria-label]"))
        .some((node) => node.getAttribute("aria-label")?.startsWith("Agent effort:")));
      const submit = Array.from(dialog.querySelectorAll("button")).find((node) => node.textContent?.trim() === "Start session");
      assert(submit);
      await act(async () => submit.click());
      await act(async () => document.querySelector('[data-testid="new-session-picker"]')?.dispatchEvent(new Event("animationend", { bubbles: true })));
      const spawns = view.requests.filter((request) => request.command === "spawn_general_agent_session");
      assert.deepEqual(spawns, [{ command: "spawn_general_agent_session", args: { params: {
        workspaceId: "general", providerId: "chosen-provider",
        model: "chosen-model", reasoningEffort: "high", preset: undefined, accountId: undefined,
        role: "general_agent", intent: "fresh_general",
      } } }]);
      assert(!view.requests.some((request) => request.command === "mux_spawn_tab"));
      const current = view.observed();
      for (const key of ["id", "title", "protected", "role", "role_profile", "cwd", "pty_id", "agent_type", "agent_session_id", "session_binding_nonce"] as const) {
        assert.equal(current.tabs[view.main.id][key], view.main[key], `Main ${key} stays unchanged`);
      }
      assert(view.host.querySelector('[data-tab-id="fresh-tab"]'), "the returned independent session is visible");
      assert(!document.body.querySelector('[role="dialog"]'), "successful spawn closes the modal");
    } finally {
      await view.close();
    }
  });

  it("shows that machine's own New session button when switched to an online remote machine and creates the session there", async () => {
    setSidebarMachineFilter({ id: "machine-x", name: "BLACKSTAR" });
    const view = await mountGeneral([
      { peerId: "machine-x", name: "BLACKSTAR", availability: "online", sessions: [] },
    ]);
    try {
      assert.equal(view.host.querySelector('[aria-label="New session on another machine"]'), null);
      const button = view.host.querySelector<HTMLButtonElement>('[data-testid="sidebar-new-session"]');
      assert(button && !button.disabled, "switched remote machine exposes an enabled New session button");
      assert.equal(button.closest("[hidden]"), null, "remote machine New session button is visible");
      await act(async () => button.click());
      const remoteSpawn = view.requests.find((request) => request.command === "spawn_general_agent_session" && request.args?.peerId === "machine-x");
      assert.deepEqual(remoteSpawn?.args, { peerId: "machine-x", params: { role: "general_agent", intent: "fresh_general" } });
      assert.equal(document.body.querySelector('[role="dialog"]'), null, "remote General spawn does not open the local modal");
      const sidebarState = view.host.querySelector('[data-testid="machine-filter"]');
      assert.equal(sidebarState?.getAttribute("data-value"), "machine-x", "stay on the selected remote machine");
      assert.equal(sidebarState?.getAttribute("data-focused-tab"), "mesh:machine-x/remote-fresh-tab");
      assert.equal(view.refreshPeerInventory.mock.calls.length, 1);
      assert.deepEqual(view.refreshPeerInventory.mock.calls[0], ["machine-x"]);
    } finally {
      await view.close();
    }
  });

  it.each<[GeneralAvailability, string]>([
    ["offline", "offline"],
    ["signed-out", "signed out"],
    ["update-required", "update required"],
  ])("disables New session and never spawns when switched to a %s remote machine", async (availability, label) => {
    setSidebarMachineFilter({ id: "machine-x", name: "BLACKSTAR" });
    const view = await mountGeneral([
      { peerId: "machine-x", name: "BLACKSTAR", availability, sessions: [] },
    ]);
    try {
      const button = view.host.querySelector<HTMLButtonElement>('[data-testid="sidebar-new-session"]');
      assert(button, "unavailable remote machine still renders the disabled button");
      assert.equal(button.disabled, true);
      assert.equal(button.getAttribute("aria-disabled"), "true");
      assert(button.title.includes(label), `button title names ${label}`);
      await act(async () => button.click());
      assert.equal(view.requests.filter((request) => request.command === "spawn_general_agent_session").length, 0);
    } finally {
      await view.close();
    }
  });
});

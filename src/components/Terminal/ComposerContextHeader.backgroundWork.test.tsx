import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentInspectionProvider } from "@codeterm/chat-ui";
import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { BackgroundProcessSnapshot } from "../../lib/backgroundProcesses";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ProviderDescriptor } from "../../lib/types";
import type { AutomationGraphAdapter } from "../Automation/types";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { ComposerBackgroundWorkProvider } from "./ComposerBackgroundWorkContext";
import { ComposerContextHeader } from "./ComposerContextHeader";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { tabActivity } from "../../lib/tabActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const authority = { rootThreadId: "root", bindingNonce: "nonce", epoch: 1 };

function snapshot(processes: BackgroundProcessSnapshot["processes"]): BackgroundProcessSnapshot {
  return { ...authority, processes };
}

function fixtureMessage(id: string, toolInput: Record<string, unknown>, timestamp = new Date(Date.now() - 20_000).toISOString()): NormalizedChatMessage {
  return { id, type: "tool_call", content: "", timestamp, sourceSessionId: authority.rootThreadId, toolKind: "agent", toolName: "agent_activity", toolInput };
}

const adapter: AutomationGraphAdapter = {
  read: async (scope) => ({ workspace_id: scope.workspace_id, team_id: null, all_teams: true, revision: "r", scenarios: [], nodes: [], edges: [], blueprints: [] }),
  subscribe: async () => () => undefined,
  stop: async () => undefined,
  delete: async () => undefined,
};

function harness(messages: readonly NormalizedChatMessage[], onInspect: (threadId: string) => void, background?: { running: Record<string, string>; unknown?: Record<string, string>; ended?: Record<string, string> }) {
  const state = {
    workspaces: [],
    tabs: { "tab-a": { provider_signals: { background }, agent_type: "codex", agent_session_id: authority.rootThreadId, session_binding_nonce: authority.bindingNonce, agent_runtime_state: { state:"loaded", epoch: authority.epoch } } },
    teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  } as unknown as ExtendedState;
  const store = createMuxStateStore(() => state);
  const host = document.createElement("div");
  const root = createRoot(host);
  const render = () => root.render(
    <MuxStateStoreContext.Provider value={store}>
      <AgentInspectionProvider onInspect={onInspect}>
        <ComposerBackgroundWorkProvider tabId="tab-a" messages={messages}>
          <AgentStateIndicator activity={tabActivity({agent_session_id:"root",agent_state:{state:"working"}})} activeTool={messages.find(message=>message.type === "tool_call" && message.toolKind === "command")} providerName="fixture" model="fixture-model" renderIdentity={()=> <button type="button" aria-label="Choose fixture model" />} />
          <ComposerContextHeader
            tabId="tab-a" workspaceId={null} delayed={[]} adapter={adapter}
            onEditDelayed={() => undefined} onDeleteDelayed={() => undefined} onScheduleDelayed={() => undefined}
          />
        </ComposerBackgroundWorkProvider>
      </AgentInspectionProvider>
    </MuxStateStoreContext.Provider>,
  );
  return { host, root, render };
}

it("opens the shared background panel and stops a terminal once", async () => {
  const process = { threadId: "root", processId: "process-1", itemId: "terminal-item", command: "node build.mjs", cwd: "C:/work", osPid: 42 };
  const terminalMessage: NormalizedChatMessage = {
    id: "terminal-item", type: "tool_call", toolKind:"command", toolStatus:"running", toolInput:{command:process.command}, content: "build output", timestamp: new Date(Date.now() - 20_000).toISOString(), sourceSessionId: authority.rootThreadId,
  };
  const stopCalls: unknown[] = [];
  let reads = 0;
  let processes = [process];
  configureOrchestrationTransport(orchestrationTestClient(async <T,>(method: string, path: string) => {
    expect(method).toBe("GET");
    expect(path).toBe("/api/crons");
    return [] as T;
  }));
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setListenForTest(async () => () => undefined);
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "agent_runtime_background_processes") { reads++; return snapshot(processes) as T; }
    if (command === "agent_runtime_stop_background_process") {
      stopCalls.push(args);
      processes = [];
      return snapshot(processes) as T;
    }
    throw new Error(`unexpected invoke: ${command}`);
  });
  setAgentDescriptors([{ id: "codex", agent_runtime: { backgroundProcesses: true } }] as ProviderDescriptor[]);
  const view = harness([terminalMessage], vi.fn());
  document.body.append(view.host);
  try {
    await act(async () => { view.render(); });
    await act(async () => { await Promise.resolve(); });
    const linked = view.host.querySelector('[role="group"][aria-label="Linked to this tab"]');
    const bubble = view.host.querySelector<HTMLButtonElement>('button[aria-label="1 background work"]');
    expect(linked).not.toBeNull();
    expect(bubble?.closest('[role="group"][aria-label="Linked to this tab"]')).toBe(linked);
    await act(async () => { bubble?.click(); });
    const panel = view.host.querySelector<HTMLElement>('[role="group"][aria-label="Background work"]');
    expect(panel).not.toBeNull();
    const panelStyle = window.getComputedStyle(panel!);
    expect(panelStyle.maxHeight).toBe("200px");
    expect(panelStyle.overflowY).toBe("auto");
    expect(panelStyle.overscrollBehaviorY).toBe("contain");
    expect(panelStyle.touchAction).toBe("pan-y");
    expect(panel?.tabIndex).toBe(0);
    expect(view.host.querySelectorAll('button[aria-label="Details for build.mjs"]')).toHaveLength(1);
    expect(reads).toBe(1);
    expect(panel?.querySelector("code,pre")).toBeNull();
    expect(view.host.querySelector('button[aria-label="Choose fixture model"]')).not.toBeNull();
    expect(view.host.querySelector('[data-testid="agent-state"]')?.textContent).not.toContain(process.command);
    const details=panel!.querySelector<HTMLButtonElement>('button[aria-label="Details for build.mjs"]')!;
    await act(async()=>details.click());
    expect(details.getAttribute("aria-expanded")).toBe("true");
    expect(panel?.querySelector('[role="region"] code')?.textContent).toBe(process.command);
    await act(async()=>details.click());
    expect(panel?.querySelector('[role="region"]')).toBeNull();
    const stop = panel?.querySelector<HTMLButtonElement>('button[aria-label="Stop build.mjs"]');
    expect(stop).not.toBeNull();
    await act(async () => { stop?.click(); stop?.click(); });
    expect(stopCalls).toHaveLength(1);
    expect(stopCalls[0]).toMatchObject({ tabId: "tab-a", request: { ...authority, threadId: "root", processId: "process-1" } });
    expect(view.host.querySelector('button[aria-label="1 background work"]')).toBeNull();
  } finally {
    await act(async () => view.root.unmount());
    view.host.remove();
  }
});

it("counts provider-declared subagents in the shared bubble and inspects every child", async () => {
  const message = fixtureMessage("agent-item", {
    agent_id: "child-thread", agent_call_id: "call-1", agent_parent_thread_id: "root", agent_ordinal: 1,
    agent_state: "running", agent_description: "Inspect the launch path", agent_model: "model-x", agent_reasoning_effort: "high", agent_binding_nonce:authority.bindingNonce, agent_runtime_epoch:authority.epoch,
  });
  const inspect = vi.fn();
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", supports_subagent_visibility: false }] as ProviderDescriptor[]);
  const hidden = harness([message], inspect);
  try {
    await act(async () => { hidden.render(); });
    expect(hidden.host.querySelector('[aria-label="Background work"]')).toBeNull();
  } finally {
    await act(async () => hidden.root.unmount());
  }

  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness", supports_subagent_visibility: true }] as ProviderDescriptor[]);
  const messages = [message, ...[2, 3, 4].map(index => fixtureMessage(`agent-${index}`, { ...message.toolInput as Record<string, unknown>, agent_id: `child-${index}`, agent_call_id: `call-${index}`, agent_description: `Task ${index}`, agent_state: "unknown" }))];
  const shown = harness(messages, inspect);
  try {
    await act(async () => { shown.render(); });
    const linked = shown.host.querySelector('[role="group"][aria-label="Linked to this tab"]');
    const bubble = linked?.querySelector<HTMLButtonElement>('button[aria-label="1 background work, Internal agent of Fixture Harness"]');
    expect(bubble).not.toBeNull();
    expect(shown.host.querySelectorAll('button[aria-label^="Open transcript for "]')).toHaveLength(0);
    await act(async () => { bubble?.click(); });
    const panel = shown.host.querySelector('[role="group"][aria-label="Background work"]');
    const list = panel?.querySelector('ul[aria-label="Subagents"]');
    expect(list?.querySelectorAll("li")).toHaveLength(4);
    const transcripts = Array.from(list?.querySelectorAll<HTMLButtonElement>('button[aria-label^="Open transcript for "]') ?? []);
    expect(transcripts).toHaveLength(4);
    expect(shown.host.querySelectorAll('button[aria-label^="Open transcript for "]')).toHaveLength(4);
    for (const transcript of transcripts) {
      expect(transcript.getAttribute("aria-label")).toContain("Internal agent of Fixture Harness");
      expect(transcript.title).toContain("Internal agent of Fixture Harness");
      expect(transcript.disabled).toBe(false);
      await act(async () => { transcript.click(); });
    }
    expect(inspect.mock.calls).toEqual([["child-thread"], ["child-2"], ["child-3"], ["child-4"]]);
    await act(async () => { bubble?.click(); });
    expect(shown.host.querySelector('[aria-label="Subagents"]')).toBeNull();
  } finally {
    await act(async () => shown.root.unmount());
  }
});

it("keeps mixed background work distinct from harness-internal agents", async () => {
  const process = { threadId: "root", processId: "process-mixed", itemId: "mixed-terminal", command: "node mixed.mjs", cwd: "C:/work", osPid: 43 };
  const command: NormalizedChatMessage = { id: process.itemId, type: "tool_call", toolKind: "command", toolStatus: "running", content: "", timestamp: new Date(Date.now() - 20_000).toISOString(), sourceSessionId: "root" };
  const child = fixtureMessage("mixed-agent", { agent_id: "mixed-child", agent_parent_thread_id: "root", agent_state: "unknown", agent_description: "Child task" });
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setListenForTest(async () => () => undefined);
  __setInvokeForTest(async <T,>(name: string) => {
    if (name === "agent_runtime_background_processes") return snapshot([process]) as T;
    throw new Error(`Unexpected invoke: ${name}`);
  });
  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness", supports_subagent_visibility: true, agent_runtime: { backgroundProcesses: true } }] as ProviderDescriptor[]);
  const inspect = vi.fn();
  const view = harness([command, child], inspect);
  try {
    await act(async () => { view.render(); });
    const bubble = view.host.querySelector<HTMLButtonElement>('button[aria-label="1 background work"]');
    expect(bubble).not.toBeNull();
    expect(bubble!.title).toBe("Background work");
    await act(async () => { bubble!.click(); });
    const panel = view.host.querySelector('[role="group"][aria-label="Background work"]');
    expect(panel?.querySelector('button[aria-label="Details for mixed.mjs"]')).not.toBeNull();
    const transcript = panel?.querySelector<HTMLButtonElement>('button[aria-label="Open transcript for Child task, Internal agent of Fixture Harness"]');
    expect(transcript).not.toBeNull();
    await act(async () => { transcript!.click(); });
    expect(inspect).toHaveBeenCalledExactlyOnceWith("mixed-child");
  } finally {
    await act(async () => view.root.unmount());
  }
});

it("shows no background row when nothing is running, even with dropped or ended tasks", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness" }] as ProviderDescriptor[]);
  const view = harness([], vi.fn(), { running: {}, unknown: { dropped: "Dropped task" }, ended: { completed: "Ended task" } });
  try {
    await act(async () => { view.render(); });
    expect(view.host.querySelector('button[aria-label^="0 background work"]')).toBeNull();
    expect(view.host.querySelector('[aria-label="Background work"]')).toBeNull();
  } finally {
    await act(async () => view.root.unmount());
  }
});

it("renders each live task as one bubble with its state inside and a readable name", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness" }] as ProviderDescriptor[]);
  const view = harness([], vi.fn(), { running: { bm76h90j0: "", a2: "Run the release gate" }, unknown: { dropped: "Dropped task" }, ended: { completed: "Ended task" } });
  try {
    await act(async () => { view.render(); });
    const chip = view.host.querySelector<HTMLButtonElement>('button[aria-label="2 background work, Internal agent of Fixture Harness"]');
    expect(chip).not.toBeNull();
    await act(async () => { chip!.click(); });
    const panel = view.host.querySelector<HTMLElement>('[role="group"][aria-label="Background work"]')!;
    const bubbles = Array.from(panel.querySelectorAll<HTMLButtonElement>('button[aria-label^="Open transcript for "]'));
    expect(bubbles.map((bubble) => bubble.querySelector(".ct-work-state")?.getAttribute("data-state"))).toEqual(["running", "running", "unknown"]);
    expect(bubbles.every((bubble) => bubble.closest("li")?.querySelectorAll(".ct-ctx-bubble").length === 1)).toBe(true);
    const internal = bubbles.find((bubble) => bubble.getAttribute("aria-label")?.startsWith("Open transcript for Internal agent"))!;
    expect(internal.textContent).not.toContain("bm76h90j0");
    expect(internal.querySelector(".ct-work-bubble__ordinal")?.textContent).toBe("1");
    expect(internal.getAttribute("aria-description")).toContain("Task Internal agent 1, running");
    expect(internal.getAttribute("aria-description")).toContain("id bm76h90j0");
    expect(panel.textContent).toContain("Run the release gate");
    expect(panel.querySelector('.ct-work-state[data-state="unknown"]')?.getAttribute("title")).toBe("No live evidence");
    expect(panel.textContent).not.toContain("Ended task");
  } finally {
    await act(async () => view.root.unmount());
  }
});

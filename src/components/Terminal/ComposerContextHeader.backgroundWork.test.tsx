import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentInspectionProvider } from "@codeterm/chat-ui";
import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { BackgroundProcessSnapshot, BackgroundStopResult } from "../../lib/backgroundProcesses";
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

function harness(messages: readonly NormalizedChatMessage[], onInspect: (threadId: string) => void, background?: { running: Record<string, string>; unknown?: Record<string, string>; ended?: Record<string, string>; kinds?: Record<string, "shell" | "subagent"> }, processes: BackgroundProcessSnapshot["processes"] = []) {
  let state = {
    workspaces: [],
    tabs: { "tab-a": { provider_signals: { background }, title: "FixtureAgent", agent_type: "codex", agent_session_id: authority.rootThreadId, session_binding_nonce: authority.bindingNonce, agent_runtime_state: { state:"loaded", epoch: authority.epoch, backgroundProcesses: snapshot(processes) } } },
    teams: [], active_workspace_id: null, focused_tab_id: null, notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  } as unknown as ExtendedState;
  const store = createMuxStateStore(() => state);
  const host = document.createElement("div");
  const root = createRoot(host);
  const render = () => root.render(
    <MuxStateStoreContext.Provider value={store}>
      <AgentInspectionProvider onInspect={onInspect}>
        <ComposerBackgroundWorkProvider tabId="tab-a" messages={messages}>
          <AgentStateIndicator activity={tabActivity({agent_session_id:"root",agent_state:{state:"working"}}, "Working")} activeTool={messages.find(message=>message.type === "tool_call" && message.toolKind === "command")} providerName="fixture" model="fixture-model" renderIdentity={()=> <button type="button" aria-label="Choose fixture model" />} />
          <ComposerContextHeader
            tabId="tab-a" workspaceId={null} delayed={[]} adapter={adapter}
            onEditDelayed={() => undefined} onDeleteDelayed={() => undefined} onScheduleDelayed={() => undefined}
          />
        </ComposerBackgroundWorkProvider>
      </AgentInspectionProvider>
    </MuxStateStoreContext.Provider>,
  );
  const setProcesses = (processes: BackgroundProcessSnapshot["processes"]) => {
    const tab = { ...state.tabs["tab-a"], agent_runtime_state: { state: "loaded" as const, epoch: authority.epoch, backgroundProcesses: snapshot(processes) } };
    state = { ...state, tabs: { ...state.tabs, "tab-a": tab } };
    store.notify();
  };
  const setProviderRoster = (background: NonNullable<ExtendedState["tabs"][string]["provider_signals"]>["background"]) => {
    const tab = { ...state.tabs["tab-a"], provider_signals: { auth: null, api_error: null, permission_mode: null, background } };
    state = { ...state, tabs: { ...state.tabs, "tab-a": tab } };
    store.notify();
  };
  return { host, root, render, setProcesses, setProviderRoster };
}

it("views provider process output and asks the bound agent to stop it until completion arrives", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness" }] as ProviderDescriptor[]);
  const command: NormalizedChatMessage = {
    id: "shell-call", type: "tool_call", content: "", sourceSessionId: "root",
    toolKind: "command", toolInput: { command: "node fixture.mjs" }, toolId: "call", toolCallSourceId: "source",
  };
  const result: NormalizedChatMessage = {
    id: "shell-result", type: "tool_result", content: "", sourceSessionId: "root",
    toolId: "call", toolCallSourceId: "source", toolPairing: "exact", toolBackgroundTaskId: "opaque-shell",
  };
  const calls: unknown[] = [];
  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push([command, args]);
    return (command === "agent_provider_background_output" ? { data: "fixture output" } : "written") as T;
  });
  const view = harness([command, result], vi.fn(), { running: { "opaque-shell": "" }, kinds: { "opaque-shell": "shell" } });
  try {
    await act(async () => view.render());
    const chip = view.host.querySelector<HTMLButtonElement>('button[aria-label="Background process · fixture.mjs"]');
    expect(chip).not.toBeNull();
    await act(async () => chip!.click());
    // Secondary actions live behind the chevron, not in an always-visible row under the pill.
    expect(view.host.querySelector('button[aria-label="View output for fixture.mjs"]')).toBeNull();
    const chevron = view.host.querySelector<HTMLButtonElement>('button[aria-label="Details for fixture.mjs"]')!;
    expect(chevron).not.toBeNull();
    await act(async () => chevron.click());
    expect(chevron.getAttribute("aria-expanded")).toBe("true");
    expect(view.host.querySelector("code")?.textContent).toBe("node fixture.mjs");
    expect(view.host.textContent).toContain("Stop unavailable");
    const output = view.host.querySelector<HTMLButtonElement>('button[aria-label="View output for fixture.mjs"]')!;
    await act(async () => output.click());
    expect(view.host.querySelector("pre")?.textContent).toBe("fixture output");
    const ask = Array.from(view.host.querySelectorAll("button")).find(button => button.textContent === "Ask FixtureAgent to stop it");
    expect(ask).toBeDefined();
    await act(async () => { ask!.click(); ask!.click(); });
    expect(view.host.textContent).toContain("Stop request written");
    expect(view.host.contains(chip)).toBe(true);
    const bound = { tabId: "tab-a", request: { sessionId: "root", bindingNonce: "nonce", taskId: "opaque-shell" } };
    expect(calls).toEqual([
      ["agent_provider_background_output", bound],
      ["agent_provider_ask_stop_background_work", bound],
    ]);
    await act(async () => view.setProviderRoster({
      running: {}, ended: { "opaque-shell": "" }, kinds: { "opaque-shell": "shell" },
      overflow: false, history_incomplete: false, history_initialized: true,
    }));
    expect(view.host.querySelector('button[aria-label="Background process · fixture.mjs"]')).toBeNull();
  } finally {
    await act(async () => view.root.unmount());
    __setInvokeForTest(null);
  }
});

it.each(["stopped", "degraded"] as const)("stops a terminal once and renders the %s outcome", async (status) => {
  const process = { threadId: "root", processId: "process-1", itemId: "terminal-item", command: "node build.mjs", cwd: "C:/work", osPid: 42 };
  const terminalMessage: NormalizedChatMessage = {
    id: "terminal-item", type: "tool_call", toolKind:"command", toolStatus:"running", toolInput:{command:process.command}, content: "build output", timestamp: new Date().toISOString(), sourceSessionId: authority.rootThreadId,
  };
  const stopCalls: unknown[] = [];
  configureOrchestrationTransport(orchestrationTestClient(async <T,>(method: string, path: string) => {
    expect(method).toBe("GET");
    expect(path).toBe("/api/crons");
    return [] as T;
  }));
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setListenForTest(async () => () => undefined);
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "agent_runtime_stop_background_process") {
      stopCalls.push(args);
      const result: BackgroundStopResult = status === "stopped"
        ? { status, snapshot: snapshot([]) }
        : { status, reason: "attributionUnavailable" };
      return result as T;
    }
    throw new Error(`unexpected invoke: ${command}`);
  });
  setAgentDescriptors([{ id: "codex", agent_runtime: { backgroundProcesses: true } }] as ProviderDescriptor[]);
  const view = harness([terminalMessage], vi.fn(), undefined, [process]);
  document.body.append(view.host);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    await act(async () => { view.render(); });
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(750); });
    const linked = view.host.querySelector('[role="group"][aria-label="Linked to this tab"]');
    const bubble = view.host.querySelector<HTMLButtonElement>('button[aria-label="Background process · build.mjs"]');
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
    expect(panel?.querySelector("code,pre")).toBeNull();
    expect(view.host.querySelector('button[aria-label="Choose fixture model"]')).not.toBeNull();
    expect(view.host.querySelector('[data-testid="agent-state"]')?.textContent).not.toContain(process.command);
    const details = panel!.querySelector<HTMLButtonElement>('button[aria-label="Details for build.mjs"]')!;
    expect(details).not.toBeNull();
    await act(async () => details.click());
    expect(details.getAttribute("aria-expanded")).toBe("true");
    expect(panel?.querySelector('[role="region"] code')?.textContent).toBe(process.command);
    expect(view.host.querySelectorAll('button[aria-label="View output for build.mjs"]')).toHaveLength(1);
    await act(async () => details.click());
    expect(panel?.querySelector('[role="region"]')).toBeNull();
    const stop = panel?.querySelector<HTMLButtonElement>('button[aria-label="Stop build.mjs"]');
    expect(stop).not.toBeNull();
    await act(async () => { stop?.click(); stop?.click(); });
    expect(stopCalls).toHaveLength(1);
    expect(stopCalls[0]).toMatchObject({ tabId: "tab-a", request: { ...authority, threadId: "root", processId: "process-1" } });
    await act(async () => view.setProcesses([]));
    expect(view.host.querySelector('button[aria-label="Background process · build.mjs"]')).toBeNull();
    const result = view.host.querySelector('[role="alert"][aria-label="Background stop unconfirmed"]');
    if (status === "degraded") {
      expect(result?.textContent).toContain("Could not identify the OS processes for build.mjs.");
      await act(async () => result?.querySelector("button")?.click());
      expect(view.host.querySelector('[role="alert"][aria-label="Background stop unconfirmed"]')).toBeNull();
    } else {
      expect(result).toBeNull();
    }
  } finally {
    await act(async () => view.root.unmount());
    vi.useRealTimers();
    view.host.remove();
  }
});

it("omits a short-lived terminal chip and reveals one that remains in the roster", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", agent_runtime: { backgroundProcesses: true } }] as ProviderDescriptor[]);
  const process = { threadId: "root", processId: "burst", itemId: "terminal", command: "echo fixture", cwd: "C:/work", osPid: null };
  const view = harness([], vi.fn(), undefined, [process]);
  const chip = () => view.host.querySelector('button[aria-label^="Background process"]');
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    await act(async () => view.render());
    await act(async () => { await vi.advanceTimersByTimeAsync(749); });
    expect(chip()).toBeNull();
    await act(async () => view.setProcesses([]));
    await act(async () => { await vi.advanceTimersByTimeAsync(750); });
    expect(chip()).toBeNull();
    await act(async () => view.setProcesses([{ ...process, processId: "persistent" }]));
    await act(async () => { await vi.advanceTimersByTimeAsync(750); });
    expect(chip()).not.toBeNull();
  } finally {
    await act(async () => view.root.unmount());
    vi.useRealTimers();
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
    const bubble = linked?.querySelector<HTMLButtonElement>('button[aria-label="1 background work, Sub-agent of Fixture Harness"]');
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
      expect(transcript.getAttribute("aria-label")).toContain("Sub-agent of Fixture Harness");
      expect(transcript.title).toContain("Sub-agent of Fixture Harness");
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

it("keeps mixed background work distinct from provider sub-agents", async () => {
  const process = { threadId: "root", processId: "process-mixed", itemId: "mixed-terminal", command: "node mixed.mjs", cwd: "C:/work", osPid: 43 };
  const command: NormalizedChatMessage = { id: process.itemId, type: "tool_call", toolKind: "command", toolStatus: "running", content: "", timestamp: new Date(Date.now() - 20_000).toISOString(), sourceSessionId: "root" };
  const child = fixtureMessage("mixed-agent", { agent_id: "mixed-child", agent_parent_thread_id: "root", agent_state: "unknown", agent_description: "Child task" });
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness", supports_subagent_visibility: true, agent_runtime: { backgroundProcesses: true } }] as ProviderDescriptor[]);
  const inspect = vi.fn();
  const view = harness([command, child], inspect, undefined, [process]);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    await act(async () => { view.render(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(750); });
    const bubble = view.host.querySelector<HTMLButtonElement>('button[aria-label="Background process · mixed.mjs"]');
    expect(bubble).not.toBeNull();
    expect(bubble!.title).toContain("Background process · mixed.mjs");
    await act(async () => { bubble!.click(); });
    const panel = view.host.querySelector('[role="group"][aria-label="Background work"]');
    expect(panel?.querySelector('button[aria-label="Details for mixed.mjs"]')).not.toBeNull();
    const transcript = panel?.querySelector<HTMLButtonElement>('button[aria-label="Open transcript for Child task, Sub-agent of Fixture Harness"]');
    expect(transcript).not.toBeNull();
    await act(async () => { transcript!.click(); });
    expect(inspect).toHaveBeenCalledExactlyOnceWith("mixed-child");
  } finally {
    await act(async () => view.root.unmount());
    vi.useRealTimers();
  }
});

it("shows no background row when nothing is running, even with dropped or ended tasks", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "codex", display_name: "Fixture Harness" }] as ProviderDescriptor[]);
  const view = harness([], vi.fn(), { running: {}, unknown: { dropped: "Dropped task" }, ended: { completed: "Ended task" }, kinds: { bm76h90j0: "subagent", a2: "subagent", dropped: "subagent" } });
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
  const view = harness([], vi.fn(), { running: { bm76h90j0: "", a2: "Run the release gate" }, unknown: { dropped: "Dropped task" }, ended: { completed: "Ended task" }, kinds: { bm76h90j0: "subagent", a2: "subagent", dropped: "subagent" } });
  try {
    await act(async () => { view.render(); });
    const chip = view.host.querySelector<HTMLButtonElement>('button[aria-label="2 background work, Sub-agent of Fixture Harness"]');
    expect(chip).not.toBeNull();
    await act(async () => { chip!.click(); });
    const panel = view.host.querySelector<HTMLElement>('[role="group"][aria-label="Background work"]')!;
    const bubbles = Array.from(panel.querySelectorAll<HTMLButtonElement>('button[aria-label^="Open transcript for "]'));
    expect(bubbles.map((bubble) => bubble.querySelector(".ct-work-state")?.getAttribute("data-state"))).toEqual(["running", "running", "unknown"]);
    expect(bubbles.every((bubble) => bubble.closest("li")?.querySelectorAll(".ct-ctx-bubble").length === 1)).toBe(true);
    const internal = bubbles.find((bubble) => bubble.getAttribute("aria-label")?.startsWith("Open transcript for Sub-agent"))!;
    expect(internal.textContent).not.toContain("bm76h90j0");
    expect(internal.querySelector(".ct-work-bubble__ordinal")?.textContent).toBe("1");
    expect(internal.getAttribute("aria-description")).toContain("Task Sub-agent 1, running");
    expect(internal.getAttribute("aria-description")).toContain("id bm76h90j0");
    expect(panel.textContent).toContain("Run the release gate");
    expect(panel.querySelector('.ct-work-state[data-state="unknown"]')?.getAttribute("title")).toBe("No live evidence");
    expect(panel.textContent).not.toContain("Ended task");
  } finally {
    await act(async () => view.root.unmount());
  }
});

it("surfaces OS-observed runtime background commands above the composer on an idle turn without protocol capability and clears on completion", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "fixture-runtime", display_name: "Fixture Runtime", agent_runtime: {} }] as ProviderDescriptor[]);
  const process = {
    threadId: authority.rootThreadId,
    processId: "os:5000:100",
    itemId: "os-bg:5000:100",
    command: "cargo nextest run -p codeterm-service-core",
    cwd: "/work",
    osPid: 5000,
  };
  const view = harness([], vi.fn(), undefined, [process]);
  try {
    await act(async () => view.render());
    const linked = view.host.querySelector('[role="group"][aria-label="Linked to this tab"]');
    const bubble = linked?.querySelector<HTMLButtonElement>('button[aria-label^="Background process ·"]');
    expect(bubble).not.toBeNull();
    await act(async () => bubble!.click());
    const panel = view.host.querySelector<HTMLElement>('[role="group"][aria-label="Background work"]');
    expect(panel).not.toBeNull();
    const details = panel?.querySelector<HTMLButtonElement>('button[aria-label^="Details for "]');
    expect(details).not.toBeNull();
    await act(async () => details!.click());
    expect(panel?.querySelector("code")?.textContent).toBe(process.command);
    await act(async () => view.setProcesses([]));
    expect(view.host.querySelector('button[aria-label^="Background process ·"]')).toBeNull();
  } finally {
    await act(async () => view.root.unmount());
  }
});

it("renders one grouped card with helper badge, flat process list, stopping state, and +N more overflow", async () => {
  configureOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  setAgentDescriptors([{ id: "fixture-runtime", display_name: "Fixture Runtime", agent_runtime: {} }] as ProviderDescriptor[]);
  let resolveStop!: (value: BackgroundStopResult) => void;
  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(command: string) => {
    if (command === "agent_runtime_stop_background_process") {
      return await new Promise<BackgroundStopResult>((resolve) => { resolveStop = resolve; }) as T;
    }
    throw new Error(`unexpected invoke: ${command}`);
  });
  const grouped = {
    threadId: authority.rootThreadId,
    processId: "os:41208:50",
    itemId: "os-bg:41208:50",
    command: "bash scripts/build.sh check 2>&1 | tee -a /tmp/build-check.log",
    cwd: "/repo",
    osPid: 41208,
    children: [
      { command: "bash scripts/build.sh check", osPid: 41231 },
      { command: "tee -a /tmp/build-check.log", osPid: 41209 },
      { command: "sccache", osPid: 40977 },
    ],
  };
  const view = harness([], vi.fn(), undefined, [grouped]);
  try {
    await act(async () => view.render());
    const chip = view.host.querySelector<HTMLButtonElement>('button[aria-label="Background process · build.sh check"]');
    expect(chip).not.toBeNull();
    await act(async () => chip!.click());
    const panel = view.host.querySelector<HTMLElement>('[role="group"][aria-label="Background work"]')!;
    const cards = panel.querySelectorAll("li.ct-background-command");
    expect(cards).toHaveLength(1);
    const title = panel.querySelector(".ct-ctx-bubble__title");
    expect(title?.textContent).toBe("build.sh check");
    const badge = panel.querySelector<HTMLElement>('.ct-work-bubble__ordinal[aria-label="3 helper processes"]');
    expect(badge?.textContent).toBe("+3");
    const stopBtn = panel.querySelector<HTMLButtonElement>('button[aria-label="Stop build.sh check and 3 helpers"]')!;
    expect(stopBtn).not.toBeNull();
    const chevron = panel.querySelector<HTMLButtonElement>('button[aria-label="Details for build.sh check"]')!;
    await act(async () => chevron.click());
    const rows = Array.from(panel.querySelectorAll("li.ct-background-command__proc"));
    expect(rows).toHaveLength(4);
    expect(rows[0].textContent).toContain("pid 41208");
    expect(rows[1].textContent).toContain("helper · pid 41231");
    expect(rows[2].textContent).toContain("helper · pid 41209");
    expect(rows[3].textContent).toContain("helper · pid 40977");
    expect(panel.textContent).toContain("Stop ends all 4");
    expect(panel.querySelector('button[aria-label="View output for build.sh check"]')).not.toBeNull();

    // Clicking Stop enters muted stopping state and disables stop without spinner.
    await act(async () => { stopBtn.click(); });
    expect(panel.querySelector('.ct-work-state[data-state="stopping"]')?.textContent).toBe("stopping…");
    expect(stopBtn.disabled).toBe(true);
    await act(async () => { resolveStop({ status: "stopped", snapshot: snapshot([]) }); });

    // >6 processes caps the visible list at 6 rows plus a "+2 more" overflow row, and 2 groups show "2 background processes".
    const manyHelpers = {
      ...grouped,
      children: Array.from({ length: 7 }, (_, index) => ({ command: `rustc --crate-${index}`, osPid: 42000 + index })),
    };
    const secondGroup = {
      threadId: authority.rootThreadId,
      processId: "os:50000:60",
      itemId: "os-bg:50000:60",
      command: "pnpm dev",
      cwd: "/repo",
      osPid: 50000,
    };
    await act(async () => view.setProcesses([manyHelpers, secondGroup]));
    expect(view.host.querySelector('button[aria-label="2 background processes"]')).not.toBeNull();
    expect(panel.querySelector(".ct-background-command__proc--more")?.textContent).toBe("+2 more");

    // Unknown shell item riding beside a live command shows "No live evidence for this process." inside details.
    await act(async () => view.setProviderRoster({
      running: {}, unknown: { "stale-shell": "cargo check" }, ended: {}, kinds: { "stale-shell": "shell" },
      overflow: false, history_incomplete: false, history_initialized: true,
    }));
    const staleChevron = panel.querySelector<HTMLButtonElement>('button[aria-label="Details for cargo check"]')!;
    expect(staleChevron).not.toBeNull();
    await act(async () => staleChevron.click());
    expect(panel.textContent).toContain("No live evidence for this process.");
  } finally {
    await act(async () => view.root.unmount());
    __setInvokeForTest(null);
  }
});

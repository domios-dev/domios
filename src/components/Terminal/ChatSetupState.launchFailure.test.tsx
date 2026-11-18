import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { restoreStageView, transcriptLoadingStageView } from "../../lib/restoreStageView";
import type { Tab } from "../../lib/types";
import { ChatSetupState } from "./ChatSetupState";
import { chatLaunchFailureDescription, chatSetupView } from "./chatSetupModel";
import { chatBodyView, composerVisible } from "./chatBodyVisibility";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { tabActivity } from "../../lib/tabActivity";
import { takeNeighborFocus } from "../Neighbors/neighborFocus";

const reason = "SessionBindingNotObserved: launch readiness deadline elapsed";
const starting: Tab = { id: "launch-fixture", title: "Fixture", cwd: "/fixture", pty_id: "own-pty", tab_type: "Terminal",
  agent_type: "fixture", backing: { kind: "pty" }, boot_stage: "agent_launching", agent_booting: true };
const failed: Tab = { ...starting, boot_stage: "spawn_failed", boot_failure_reason: reason };

it.each([false, true])("opens the terminal for a pre-session PTY prompt with retained history %s", async (retained) => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const waiting: Tab = { ...starting, boot_stage: "agent_ready", pending_prompt_kind: "provider",
    pending_prompt: { question: "Opaque startup question", options: [] },
    session_binding_nonce: "current-launch",
    ...(retained ? { agent_session_id: "retained-session", session_bound: true,
      reported_session_binding_nonce: "previous-launch" } : {}) };
  const openTerminal = vi.fn();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = async (tab: Tab, attached: boolean) => {
    const view = chatBodyView({
      snapshotReady: attached, hasTranscript: false, chatLoading: !attached, unavailable: null,
      bootView: chatSetupView(tab, attached),
    });
    expect(composerVisible(view)).toBe(attached);
    await act(async () => root.render(<ChatSetupState tab={tab} view={view} providerName="Fixture" onOpenTerminal={openTerminal} />));
  };
  try {
    await render(waiting, false);
    expect(host.querySelector("[role=status]")).not.toBeNull();
    expect(host.querySelector("[role=progressbar]")).toBeNull();
    const button = [...host.querySelectorAll("button")].find(item => item.textContent === "Open terminal");
    await act(async () => button!.click());
    expect(openTerminal).toHaveBeenCalledOnce();
    await render({ ...waiting, agent_session_id: "own-session", session_bound: true,
      reported_session_binding_nonce: "current-launch" }, true);
    expect(host.querySelector("button")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("shows the launch provider question, answers exact values, and opens the terminal without cancel keys", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const prompt = { question: "Opaque external file decision", options: [
    { label: "Decline external file", value: "decline_opaque" },
    { label: "Allow external file", value: "allow_opaque" },
  ] };
  const waiting: Tab = { ...starting, boot_stage: "awaiting_owner_prompt", pending_prompt_kind: "provider", pending_prompt: prompt };
  const openTerminal = vi.fn();
  const cancel = vi.fn();
  const invoke = vi.fn(async () => undefined);
  __setInvokeForTest(invoke as unknown as Parameters<typeof __setInvokeForTest>[0]);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>
      <ChatSetupState tab={waiting} view={chatSetupView(waiting)} providerName="Fixture" onOpenTerminal={openTerminal} />
      <AgentStateIndicator tab={waiting} activity={tabActivity(waiting)} onCancel={cancel} />
    </>));
    expect(host.textContent).toContain(prompt.question);
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(host.textContent).not.toMatch(/Connecting|Agent may be waiting/);
    expect(host.querySelector('[aria-label="Cancel the terminal question"]')).toBeNull();
    const button = (label: string) => [...host.querySelectorAll("button")].find(item => item.textContent === label)!;
    await act(async () => button("Open terminal").click());
    expect(openTerminal).toHaveBeenCalledOnce();
    for (const option of prompt.options) {
      const next = { ...waiting, pending_prompt: { ...prompt, question: prompt.question + option.value } };
      await act(async () => root.render(<ChatSetupState tab={next} view={chatSetupView(next)} providerName="Fixture" onOpenTerminal={openTerminal} />));
      await act(async () => button(option.label).click());
      expect(invoke).toHaveBeenLastCalledWith("mux_respond_prompt", { tabId: waiting.id, value: option.value,
        identity: { kind: "provider", options: prompt.options } });
      expect(button(option.label).disabled).toBe(true);
    }
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(cancel).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
});

it("terminal launch failure ends deadline pending, including stale wake and runtime state", () => {
  expect(chatSetupView(starting).kind).toBe("starting");
  for (const agent_runtime_state of [undefined, { state: "degraded" as const, epoch: 1, failureCode: "deadline" as const, reason }]) {
    const tab = { ...failed, agent_runtime_state };
    for (const attached of [false, true]) expect(chatSetupView(tab, attached)).toEqual({ kind: "launch_failed", reason });
    for (const wake of [{}, { pending: true }, { requested: true }]) {
      const view = restoreStageView(tab, wake);
      expect(view.phase).toBe("failed");
      expect(view.hint).toBeNull();
      expect(view.detail).toBe(reason);
    }
    expect(transcriptLoadingStageView(tab).phase).toBe("failed");
    expect(transcriptLoadingStageView(tab).label).not.toBe("Starting…");
  }
  const neutral = chatLaunchFailureDescription(failed, reason);
  expect(neutral?.pending).toBe(false);
  expect(neutral?.cause).not.toContain("waiting for input");
  expect(neutral?.raw).toBe(reason);
});

it("replaces startup in one render with host cause and working terminal and restart actions", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const openTerminal = vi.fn();
  const invoke = vi.fn(async <T,>(_command: string, _args?: Record<string, unknown>): Promise<T> => undefined as T);
  __setInvokeForTest(invoke as unknown as Parameters<typeof __setInvokeForTest>[0]);
  const render = async (tab: Tab) => {
    await act(async () => { root.render(<ChatSetupState tab={tab} view={chatSetupView(tab)} providerName="Fixture" onOpenTerminal={openTerminal} />); });
  };
  try {
    await render(starting);
    expect(host.querySelector('[role="progressbar"]')).not.toBeNull();
    await render(failed);
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-launch-failed"]')).toHaveLength(1);
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    expect(host.textContent).not.toMatch(/Starting…|Starting terminal|still pending/);
    expect(host.querySelector("details pre")?.textContent).toBe(reason);
    const question = "Opaque host-classified blocking question?";
    await render({ ...failed, pending_prompt_kind: "provider", pending_prompt: { question, options: [] } });
    expect(host.textContent).toContain(question);
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    const button = (name: string) => [...host.querySelectorAll("button")].find(item => item.getAttribute("aria-label") === name);
    await act(async () => { button("Open terminal")!.click(); });
    expect(openTerminal).toHaveBeenCalledOnce();
    await act(async () => { button("Restart the agent")!.click(); });
    expect(invoke).toHaveBeenCalledExactlyOnceWith("restart_agent_tab", { tabId: failed.id });
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
  } finally {
    await act(async () => { root.unmount(); });
    host.remove();
    __setInvokeForTest(null);
  }
});

it("opens the affected Neighbor's permissions once instead of retrying a refused chat connection", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const reason = "Opaque refused launch";
  const tab: Tab = {
    id: "concierge", title: "Concierge", cwd: "/fixture", pty_id: null, tab_type: "Terminal", agent_type: "fixture",
    neighbor_authority: { uid: "owner", friendship_id: "a".repeat(64), root_tab_id: null },
    agent_runtime_state: { state: "degraded", epoch: 1, failureCode: "provider_refused", accessFailure: "neighbor_restore_grant_required", reason },
  };
  const retry = vi.fn();
  const opened = vi.fn();
  window.addEventListener("codeterm:open-panel", opened);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ChatSetupState tab={tab} providerName="Fixture" onRetry={retry}
      view={{ kind: "attach_failed", reason: "/api/tabs/concierge/chat-raw/start -> HTTP 409" }} />));
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(host.textContent).not.toMatch(/HTTP 409|chat-raw|platform|neighbor_restore_grant_required/);
    const details = host.querySelector("details")!;
    expect(details).not.toBeNull();
    expect(details.open).toBe(false);
    expect([...host.querySelectorAll("button")].filter(button => !button.closest("details"))).toHaveLength(1);
    const review = host.querySelector<HTMLButtonElement>('[aria-label="Review concierge access"]')!;
    await act(async () => review.click());
    expect(takeNeighborFocus()).toEqual({ id: tab.neighbor_authority!.friendship_id, section: "permissions" });
    expect(opened).toHaveBeenCalledOnce();
    expect((opened.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "neighbors" });
    expect(retry).not.toHaveBeenCalled();
    await act(async () => root.render(<ChatSetupState tab={{ ...tab, agent_runtime_state: { state: "loaded", epoch: 2 } }} providerName="Fixture" view={{ kind: "none" }} />));
    expect(host.querySelector('[data-testid="chat-neighbor-access-required"]')).toBeNull();
  } finally {
    window.removeEventListener("codeterm:open-panel", opened);
    await act(async () => root.unmount());
    host.remove();
  }
});

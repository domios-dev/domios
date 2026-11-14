import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { restoreStageView, transcriptLoadingStageView } from "../../lib/restoreStageView";
import type { Tab } from "../../lib/types";
import { ChatSetupState } from "./ChatSetupState";
import { chatLaunchFailureDescription, chatSetupView } from "./chatSetupModel";

const reason = "SessionBindingNotObserved: launch readiness deadline elapsed";
const starting: Tab = { id: "launch-fixture", title: "Fixture", cwd: "/fixture", pty_id: "own-pty", tab_type: "Terminal",
  agent_type: "fixture", backing: { kind: "pty" }, boot_stage: "agent_launching", agent_booting: true };
const failed: Tab = { ...starting, boot_stage: "spawn_failed", boot_failure_reason: reason };

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
  __setInvokeForTest(invoke);
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

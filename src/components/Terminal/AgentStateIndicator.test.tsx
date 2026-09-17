import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { AgentStateIndicator } from "./AgentStateIndicator";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
Object.defineProperty(globalThis, "window", { configurable: true, value: window });
Object.defineProperty(globalThis, "document", { configurable: true, value: window.document });
Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: window.HTMLElement });
Object.defineProperty(globalThis, "SVGElement", { configurable: true, value: window.SVGElement });

async function render(ui: React.ReactElement): Promise<{ host: HTMLElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

async function main(): Promise<void> {
  const idle = await render(
    <AgentStateIndicator
      state={{ state: "idle" }}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: " high ", actual_source: "session-owned" }}
    />,
  );
  assert(idle.host.querySelectorAll('[data-testid="agent-state"]').length === 1, "idle keeps exactly one status row mounted");
  assert(idle.host.querySelectorAll(".activity-dots i").length === 3, "idle row keeps three dots");
  assert(idle.host.querySelector(".activity-dots")?.classList.contains("activity-dots--muted"), "idle dots are gray");
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol · high", "idle row includes session-owned actual effort");

  let stopCalls = 0;
  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "working", status_line: "Editing files", phase: "thinking" }}
      providerName="Codex"
      model="gpt-5.6-sol"
      onStop={() => { stopCalls += 1; }}
      reasoningEffort={{ actual: null, actual_source: "session-owned" }}
    />,
  ));
  assert(idle.host.querySelector('[data-agent-state="working"]'), "working reuses the same row");
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol · Editing files", "working row contains provider, model, and current status");
  const stop = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-stop"]');
  assert(stop, "working row preserves Stop");
  await act(async () => stop.click());
  assert(stopCalls === 1, "Stop invokes its handler once");

  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "working", status_line: "Editing files" }}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: "high", actual_source: "provider-default-unverified" }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol · Editing files", "unverified effort is omitted");

  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "idle" }}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: null, requested: "medium" }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol", "requested-only effort is omitted");

  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "idle" }}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: "   ", actual_source: "session-owned" }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol", "empty effort is omitted without a separator");

  await act(async () => idle.root.render(
    <div style={{ width: 390 }}>
      <AgentStateIndicator
        state={{ state: "working", status_line: "A deliberately long status suffix that must remain in the compact row" }}
        providerName="Codex"
        model="gpt-5.6-sol"
        reasoningEffort={{ actual: "high", actual_source: "session-owned" }}
      />
    </div>,
  ));
  assert(idle.host.querySelectorAll('[data-testid="agent-state"]').length === 1, "390px rendering keeps one compact status row");
  assert(idle.host.querySelector(".ct-agent-msg")?.classList.contains("truncate"), "status suffix remains truncatable at narrow width");

  let terminalCalls = 0;
  let cancelCalls = 0;
  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "question", subtype: "permission" }}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: "low", actual_source: "provider-default-unverified" }}
      onOpenTerminal={() => { terminalCalls += 1; }}
      onCancel={() => { cancelCalls += 1; }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol · Permission needed", "question stays in the single compact row");
  const terminal = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-open-terminal"]');
  const cancel = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-cancel-question"]');
  assert(terminal && cancel, "question row preserves terminal and cancel actions");
  await act(async () => { terminal.click(); cancel.click(); });
  assert(terminalCalls === 1 && cancelCalls === 1, "question actions invoke their handlers once");

  await act(async () => idle.root.render(
    <AgentStateIndicator state={{ state: "quiet" }} providerName="Codex" model="gpt-5.6-sol" />,
  ));
  const quiet = idle.host.querySelector('[data-testid="agent-state"]');
  assert(quiet?.getAttribute("data-agent-state") === "quiet", "quiet state keeps the status rail hook");
  assert(quiet?.getAttribute("role") === "status", "quiet state keeps the accessible status role");
  assert(quiet?.getAttribute("aria-label")?.includes("rebind"), "quiet state names the recovery action");

  await act(async () => idle.root.unmount());
  idle.host.remove();
  console.log("agent status row checks passed");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});

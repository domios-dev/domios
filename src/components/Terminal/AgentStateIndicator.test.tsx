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
  const idle = await render(<AgentStateIndicator state={{ state: "idle" }} model="gpt-5.6-sol" />);
  assert(idle.host.querySelectorAll('[data-testid="agent-state"]').length === 1, "idle keeps exactly one status row mounted");
  assert(idle.host.querySelectorAll(".activity-dots i").length === 3, "idle row keeps three dots");
  assert(idle.host.querySelector(".activity-dots")?.classList.contains("activity-dots--muted"), "idle dots are gray");
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "gpt-5.6-sol", "idle row contains model only without a summary");

  let stopCalls = 0;
  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "working", status_line: "Editing files", phase: "thinking" }}
      model="gpt-5.6-sol"
      onStop={() => { stopCalls += 1; }}
    />,
  ));
  assert(idle.host.querySelector('[data-agent-state="working"]'), "working reuses the same row");
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "gpt-5.6-sol · Editing files", "working row contains model and current status");
  const stop = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-stop"]');
  assert(stop, "working row preserves Stop");
  await act(async () => stop.click());
  assert(stopCalls === 1, "Stop invokes its handler once");

  let terminalCalls = 0;
  let cancelCalls = 0;
  await act(async () => idle.root.render(
    <AgentStateIndicator
      state={{ state: "question", subtype: "permission" }}
      model="gpt-5.6-sol"
      onOpenTerminal={() => { terminalCalls += 1; }}
      onCancel={() => { cancelCalls += 1; }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "gpt-5.6-sol · Permission needed", "question stays in the single compact row");
  const terminal = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-open-terminal"]');
  const cancel = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-cancel-question"]');
  assert(terminal && cancel, "question row preserves terminal and cancel actions");
  await act(async () => { terminal.click(); cancel.click(); });
  assert(terminalCalls === 1 && cancelCalls === 1, "question actions invoke their handlers once");

  await act(async () => idle.root.unmount());
  idle.host.remove();
  console.log("agent status row checks passed");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});

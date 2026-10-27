import { tabActivity } from "../../lib/tabActivity";
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
  const phaseRow = await render(<AgentStateIndicator activity={tabActivity({ agent_session_id: "session", agent_state: { state: "working", status_line: "opaque previous activity" } })} />);
  for (const phase of ["compacting", undefined, "compacting", "streaming"] as const) {
    const activity = tabActivity({ agent_session_id: "session", agent_state: { state: "working", phase } });
    await act(async () => phaseRow.root.render(<AgentStateIndicator activity={activity} activeTool={phase === "compacting" ? { toolKind: "command", toolDetail: "opaque previous command" } : undefined} />));
    const status = phaseRow.host.querySelector('[role="status"]');
    assert(status?.getAttribute("data-agent-phase") === activity.phase, "reported phase reaches the status view");
    assert(status?.textContent === activity.label, "phase transitions replace stale tool and stabilized labels immediately");
  }
  await act(async () => phaseRow.root.unmount());
  phaseRow.host.remove();
  const background = { auth: null, api_error: null, permission_mode: null, background: { running: { task: "shell" }, overflow: false, history_incomplete: false, history_initialized: true } };
  for (const [state, expected] of [["idle", "idle"], ["working", "working"]] as const) {
    const row = await render(
      <AgentStateIndicator
        activity={tabActivity({ agent_session_id: "session", provider_signals: background, agent_state: { state } })}
        onStop={() => {}}
      />,
    );
    assert(row.host.querySelector('[role="status"]')?.getAttribute("data-tab-activity") === expected, `background task with ${state} turn renders ${expected}`);
    assert(Boolean(row.host.querySelector('[data-testid="agent-stop"]')) === (expected === "working"), `stop is offered only for an active turn (${state})`);
    await act(async () => row.root.unmount());
    row.host.remove();
  }
  const command = '"C:\\Program Files\\PowerShell\\7\\pwsh.exe" -Command "pnpm test"';
  for (const renderIdentity of [undefined, (label: string) => <span>{label}</span>]) {
    const running = await render(
      <AgentStateIndicator
        activity={tabActivity({ agent_session_id:"session", agent_state:{state:"working",status_line:"Working"} })}
        providerName="Provider"
        renderIdentity={renderIdentity}
        activeTool={{toolKind:"command",toolDetail:command}}
      />,
    );
    assert(running.host.querySelector('[role="status"]')?.textContent?.includes("Running tests"), "running row shows the shared command label");
    assert(Array.from(running.host.querySelectorAll("[title]")).some(element => element.getAttribute("title") === command), "hover preserves the complete raw command");
    await act(async () => running.root.unmount());
    running.host.remove();
  }
  const idle = await render(
    <AgentStateIndicator
      activity={tabActivity({ agent_state: { state: "idle" } })}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: " high ", actual_source: "session-owned" }}
    />,
  );
  assert(idle.host.querySelectorAll('[data-testid="agent-state"]').length === 1, "idle keeps exactly one status row mounted");
  assert(!idle.host.querySelector('[data-testid="agent-activity-slot"]'), "an idle row with nothing to say shows no activity dots");
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol · high", "idle row includes session-owned actual effort");

  let stopCalls = 0;
  await act(async () => idle.root.render(
    <AgentStateIndicator
      activity={tabActivity({ agent_session_id: "bound-session", agent_state: { state: "working", status_line: "Editing files", phase: "thinking" } })}
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
      activity={tabActivity({ agent_state: { state: "idle" } })}
      providerName="Codex"
      model="gpt-5.6-sol"
      onStop={() => { stopCalls += 1; }}
    />,
  ));
  const slot = idle.host.querySelector<HTMLButtonElement>('[data-testid="agent-stop-slot"]');
  assert(slot, "an idle row keeps the Stop slot so the row never changes height");
  assert(slot.disabled && slot.getAttribute("aria-hidden") === "true", "the idle Stop slot is inert and hidden from assistive tech");
  assert(!idle.host.querySelector('[data-testid="agent-stop"]'), "idle offers no Stop action");

  await act(async () => idle.root.render(
    <AgentStateIndicator
      activity={tabActivity({ agent_session_id: "bound-session", agent_state: { state: "working", status_line: "Editing files" } })}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: "high", actual_source: "provider-default-unverified" }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol · Editing files", "unverified effort is omitted");

  await act(async () => idle.root.render(
    <AgentStateIndicator
      activity={tabActivity({ agent_state: { state: "idle" } })}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: null, requested: "medium" }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol", "requested-only effort is omitted");

  await act(async () => idle.root.render(
    <AgentStateIndicator
      activity={tabActivity({ agent_state: { state: "idle" } })}
      providerName="Codex"
      model="gpt-5.6-sol"
      reasoningEffort={{ actual: "   ", actual_source: "session-owned" }}
    />,
  ));
  assert(idle.host.querySelector(".ct-agent-msg")?.textContent === "Codex · gpt-5.6-sol", "empty effort is omitted without a separator");

  await act(async () => idle.root.render(
    <div style={{ width: 390 }} data-testid="phone-composer-status">
      <button type="button" aria-label="Attachment actions" />
      <AgentStateIndicator
        activity={tabActivity({ agent_session_id: "bound-session", agent_state: { state: "working", status_line: "A deliberately long status suffix that must remain in the compact row" } })}
        providerName="Codex"
        model="gpt-5.6-sol"
        onStop={() => { stopCalls += 1; }}
        reasoningEffort={{ actual: "high", actual_source: "session-owned" }}
      />
    </div>,
  ));
  assert(idle.host.querySelectorAll('[data-testid="agent-state"]').length === 1, "390px rendering keeps one compact status row");
  const phoneStatus = idle.host.querySelector('[data-testid="phone-composer-status"]');
  const phoneAttach = phoneStatus?.querySelector('button[aria-label="Attachment actions"]');
  const phoneStop = phoneStatus?.querySelector<HTMLButtonElement>('[data-testid="agent-stop"]');
  assert(phoneAttach && phoneStop, "phone status row keeps attachment actions and working Stop together");
  await act(async () => phoneStop.click());
  assert(Number(stopCalls) === 2, "phone Stop still interrupts the active agent");

  let terminalCalls = 0;
  let cancelCalls = 0;
  await act(async () => idle.root.render(
    <AgentStateIndicator
      activity={tabActivity({ agent_state: { state: "question", subtype: "permission" } })}
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
    <AgentStateIndicator activity={tabActivity({ agent_state: { state: "quiet" } })} providerName="Codex" model="gpt-5.6-sol" />,
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

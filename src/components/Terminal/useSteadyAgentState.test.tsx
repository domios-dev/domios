import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { AgentState } from "./AgentStateIndicator";
import { holdsWorkingRow, useSteadyAgentState } from "./useSteadyAgentState";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
Object.defineProperty(globalThis, "window", { configurable: true, value: window });
Object.defineProperty(globalThis, "document", { configurable: true, value: window.document });
Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: window.HTMLElement });

const working: AgentState = { state: "working", status_line: "status-token-working" };
const idle: AgentState = { state: "idle" };
const question: AgentState = { state: "question", subtype: "permission" };

assert(holdsWorkingRow(working, idle), "a working row holds through an idle gap");
assert(!holdsWorkingRow(working, question), "a question is never held back");
assert(!holdsWorkingRow(idle, working), "entering work is never held back");

let seen: AgentState | null = null;
function Probe({ state }: { state: AgentState }) {
  seen = useSteadyAgentState(state, 60_000);
  return null;
}

async function run() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<Probe state={working} />));
  assert(seen === working, "working shows immediately");
  await act(async () => root.render(<Probe state={idle} />));
  assert(seen === working, "a brief idle gap keeps the working row on screen");
  await act(async () => root.render(<Probe state={working} />));
  assert(seen === working, "resuming work inside the hold never blinks");
  await act(async () => root.render(<Probe state={question} />));
  assert(seen === question, "a question replaces the working row at once");
  await act(async () => root.unmount());
  console.log("steady agent state tests passed");
}

run().catch((error) => {
  console.error(error);
  process.exit(1);
});

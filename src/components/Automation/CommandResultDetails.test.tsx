import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { CommandResultFacts, commandOutcome, resultHandler, shownAsLastAccepted } from "./CommandResultDetails";
import { fixtureGraph } from "./fixtures.testdata";
import type { CommandResultMetadata } from "./types";

async function main() {
const window = new Window();
for (const key of ["window", "document", "HTMLElement", "navigator"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
const handler = { id: "command", type: "command", label: "Build", timeout_ms: 1000, target: null, availability: "available" as const, last_result: null };
try {
  for (const status of ["success", "nonzero", "error", "cancelled", "deadline", "unconfirmed"] as const) {
    const metadata: CommandResultMetadata = { status, exit_code: status === "success" ? 0 : null, resources_confirmed: status !== "unconfirmed", started_at: 100, finished_at: 308, stdout: { total_bytes: 10, retained_bytes: 5, truncated: true }, stderr: null };
    const result = { status: "completed", execution_id: "execution", accepted_at: 100, command_result: metadata };
    assert.equal(commandOutcome(result), status);
    await act(async () => root.render(<CommandResultFacts handlerId="command" handler={handler} result={result} />));
    const facts = new Map(Array.from(host.querySelectorAll("dt")).map((term) => [term.textContent, term.nextElementSibling?.textContent]));
    assert.equal(facts.get("Outcome"), status);
    assert.equal(facts.get("Duration"), "208 ms");
    assert.equal(facts.get("Exit code"), status === "success" ? "0" : "Unknown");
    assert.equal(facts.get("Stdout"), "10 B total · 5 B retained · truncated");
    assert.equal(facts.get("Stderr"), "Unavailable");
    assert.equal(facts.get("Resources"), status === "unconfirmed" ? "Settlement unconfirmed" : "Settled");
  }
  const graph = fixtureGraph();
  assert.equal(resultHandler(graph.nodes[0], "branch-a", "handler-a")?.type, "cron");
  assert.equal(resultHandler(graph.nodes[0], "branch-b", "handler-a"), undefined);
  assert.equal(shownAsLastAccepted(graph.nodes[0], "branch-b", graph.nodes[0].outcomes[0].handlers[0]), false);
  const malformed = { status: "success" as const, exit_code: null, resources_confirmed: false, started_at: Number.NaN, finished_at: -1, stdout: null, stderr: null };
  await act(async () => root.render(<CommandResultFacts handlerId="command" handler={handler} result={{ status: "failed", execution_id: null, accepted_at: null, command_result: malformed }} />));
  assert.ok(!host.textContent?.includes("NaN") && !host.textContent?.includes("Invalid Date"));
} finally {
  await act(async () => root.unmount());
  host.remove();
}
console.log("Command result facts passed");

}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });

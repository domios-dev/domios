import assert from "node:assert/strict";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationDetails } from "./AutomationDetails";
import { COMMAND_FACT, COMMAND_VALUE, commandResultLabel } from "./CommandResultDetails";
import { fixtureGraph, fixtureNode, sentinelIdentity } from "./fixtures.testdata";
import { automationKindLabel, type AutomationHandler, type AutomationHandlerResult, type AutomationNode, type CommandResultMetadata } from "./types";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "Event", "KeyboardEvent", "navigator"]) {
  Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const EXECUTION = "22222222-2222-4222-8222-222222222222";
const OTHER_EXECUTION = "33333333-3333-4333-8333-333333333333";
const ACCEPTED = 1791054000000;
const PRIVATE = { command: "printf command-sentinel-a", cwd: "/private/cwd-sentinel-a", env: "env-sentinel-a", stdout: "raw-stdout-sentinel-a", stderr: "raw-stderr-sentinel-a", primary: "primary-diagnostic-sentinel-a", cleanup: "cleanup-diagnostic-sentinel-a" };
const SUCCESS: CommandResultMetadata = { status: "success", exit_code: 0, resources_confirmed: true, stdout: { total_bytes: 7, retained_bytes: 7, truncated: false }, stderr: { total_bytes: 0, retained_bytes: 0, truncated: false }, started_at: ACCEPTED + 1, finished_at: ACCEPTED + 1235 };

function wire(overrides: Partial<CommandResultMetadata> = {}): CommandResultMetadata {
  const payload = { ...SUCCESS, primary: PRIVATE.primary, cleanup: [PRIVATE.cleanup], ...overrides };
  return payload;
}

function commandHandler(id: string, last_result: AutomationHandlerResult | null, label = "Write marker", timeout_ms = 1000): AutomationHandler {
  return { id, type: "command", target: null, availability: "available", label, timeout_ms, last_result };
}

function result(command_result: CommandResultMetadata | undefined, status = "completed", execution_id: string | null = EXECUTION): AutomationHandlerResult {
  return command_result === undefined ? { status, execution_id, accepted_at: ACCEPTED } : { status, execution_id, accepted_at: ACCEPTED, command_result };
}

function commandNode(handlers: readonly AutomationHandler[], last: { bucket: string; handlers: readonly (AutomationHandlerResult & { handler_id: string })[] } | null, buckets?: readonly { bucket: string; handlers: readonly AutomationHandler[] }[]): AutomationNode {
  return { ...fixtureNode(sentinelIdentity, "Command watcher"), outcomes: buckets ?? [{ bucket: "passed", handlers }], last_result: last && { ...last, accepted_at: ACCEPTED } };
}

function singleCommand(metadata: CommandResultMetadata | undefined, status = "completed", execution_id: string | null = EXECUTION): AutomationNode {
  const handlerResult = result(metadata, status, execution_id);
  return commandNode([commandHandler("marker", handlerResult)], { bucket: "passed", handlers: [{ ...handlerResult, handler_id: "marker" }] });
}

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }

async function mount(node: AutomationNode, onClose: () => void = () => undefined) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  const render = async (next: AutomationNode) => { await act(async () => { root.render(<AutomationDetails node={next} nodes={[next]} edges={[]} stale={false} stopping={false} stopFailed={false} onStop={() => undefined} onSelect={() => undefined} onClose={onClose} />); }); };
  await render(node);
  return { host, render, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}

function panel(host: HTMLElement) {
  const found = host.querySelector<HTMLElement>('aside[role="region"]');
  assert.ok(found, "details rendered");
  return found;
}

function factsOf(list: Element): Map<string, string> {
  const facts = new Map<string, string>();
  for (const term of Array.from(list.querySelectorAll("dt"))) {
    const value = term.nextElementSibling;
    assert.equal(value?.tagName, "DD", "every command fact term has its value");
    facts.set(term.textContent ?? "", value?.textContent ?? "");
  }
  return facts;
}

function commandLists(host: HTMLElement, handlerId: string) {
  const lists = Array.from(panel(host).querySelectorAll("dl")).filter((list) => list.getAttribute("aria-label") === commandResultLabel(handlerId));
  const accepted = panel(host).querySelector('ul[aria-label="Last accepted handler results"]');
  return { accepted: lists.filter((list) => accepted?.contains(list)), outcomes: lists.filter((list) => !accepted?.contains(list)) };
}

function lastAccepted(host: HTMLElement, handlerId: string) {
  const { accepted } = commandLists(host, handlerId);
  assert.equal(accepted.length, 1, `one last accepted command result for ${handlerId}`);
  return { facts: factsOf(accepted[0]), row: accepted[0].closest("li")! };
}

test("success exit zero renders supplied label, timeout, capture counts and duration under the enclosing execution", async () => {
  const view = await mount(singleCommand(wire()));
  try {
    const { facts, row } = lastAccepted(view.host, "marker");
    assert.equal(facts.get(COMMAND_FACT.label), "Write marker");
    assert.ok(facts.get(COMMAND_FACT.timeout)?.includes("1000"));
    assert.equal(facts.get(COMMAND_FACT.outcome), "success");
    assert.equal(facts.get(COMMAND_FACT.exit), "0");
    assert.equal(facts.get(COMMAND_FACT.resources), COMMAND_VALUE.confirmed);
    assert.ok(facts.get(COMMAND_FACT.stdout)?.includes("7") && facts.get(COMMAND_FACT.stdout)?.includes(COMMAND_VALUE.complete));
    assert.ok(facts.get(COMMAND_FACT.stderr)?.includes("0"));
    assert.notEqual(facts.get(COMMAND_FACT.stderr), COMMAND_VALUE.unavailable, "observed empty capture is not unavailable");
    assert.equal(facts.get(COMMAND_FACT.started), new Date(SUCCESS.started_at).toLocaleString());
    assert.equal(facts.get(COMMAND_FACT.finished), new Date(SUCCESS.finished_at).toLocaleString());
    assert.ok(facts.get(COMMAND_FACT.duration)?.includes(String(SUCCESS.finished_at - SUCCESS.started_at)));
    assert.ok(row.textContent?.includes(EXECUTION), "result stays inside its enclosing execution row");
    assert.ok(row.textContent?.includes("completed"), "outer handler status is preserved separately");
    const { outcomes } = commandLists(view.host, "marker");
    assert.equal(outcomes.length, 1);
    assert.deepEqual([...factsOf(outcomes[0]).keys()], [COMMAND_FACT.label, COMMAND_FACT.timeout], "same execution is not duplicated in possible outcomes");
  } finally { await view.close(); }
});

test("nonzero exit seven keeps truncated output counters and the outer failed status", async () => {
  const view = await mount(singleCommand(wire({ status: "nonzero", exit_code: 7, stdout: { total_bytes: 100000, retained_bytes: 32768, truncated: true }, stderr: { total_bytes: 3, retained_bytes: 3, truncated: false } }), "failed"));
  try {
    const { facts, row } = lastAccepted(view.host, "marker");
    assert.equal(facts.get(COMMAND_FACT.outcome), "nonzero");
    assert.equal(facts.get(COMMAND_FACT.exit), "7");
    const stdout = facts.get(COMMAND_FACT.stdout) ?? "";
    for (const part of ["100000", "32768", COMMAND_VALUE.truncated]) assert.ok(stdout.includes(part), `stdout carries ${part}`);
    assert.ok(facts.get(COMMAND_FACT.stderr)?.includes(COMMAND_VALUE.complete));
    assert.ok(row.textContent?.includes("failed"));
  } finally { await view.close(); }
});

test("error, cancelled and deadline stay distinct with unknown actual exit", async () => {
  const view = await mount(singleCommand(wire()));
  try {
    const seen = new Set<string>();
    for (const status of ["error", "cancelled", "deadline"] as const) {
      const captures = status === "error" ? { stdout: null, stderr: null } : {};
      await view.render(singleCommand(wire({ status, exit_code: null, ...captures }), "failed"));
      const { facts } = lastAccepted(view.host, "marker");
      assert.equal(facts.get(COMMAND_FACT.outcome), status);
      assert.equal(facts.get(COMMAND_FACT.exit), COMMAND_VALUE.unknown);
      if (status === "error") assert.equal(facts.get(COMMAND_FACT.stdout), COMMAND_VALUE.unavailable);
      seen.add(facts.get(COMMAND_FACT.outcome)!);
    }
    assert.equal(seen.size, 3);
  } finally { await view.close(); }
});

test("unconfirmed resources never read as success even with a known actual exit", async () => {
  const view = await mount(singleCommand(wire({ status: "unconfirmed", exit_code: 0, resources_confirmed: false }), "unconfirmed"));
  try {
    const { facts } = lastAccepted(view.host, "marker");
    assert.equal(facts.get(COMMAND_FACT.outcome), "unconfirmed");
    assert.equal(facts.get(COMMAND_FACT.exit), "0");
    assert.equal(facts.get(COMMAND_FACT.resources), COMMAND_VALUE.unconfirmed);
    assert.ok(![...facts.values()].includes("success"));
    assert.ok(![...facts.values()].includes(COMMAND_VALUE.confirmed));
  } finally { await view.close(); }
});

test("absent metadata, null exit and null captures stay distinct from observed zero", async () => {
  const view = await mount(singleCommand(undefined, "unconfirmed", null));
  try {
    const absent = lastAccepted(view.host, "marker");
    for (const term of [COMMAND_FACT.outcome, COMMAND_FACT.exit, COMMAND_FACT.resources, COMMAND_FACT.stdout, COMMAND_FACT.stderr, COMMAND_FACT.duration]) assert.equal(absent.facts.get(term), COMMAND_VALUE.notRecorded, `${term} is not inferred`);
    assert.ok(absent.row.textContent?.includes("unconfirmed"), "outer status survives absent metadata");
    assert.ok(!absent.row.textContent?.includes(EXECUTION));
    await view.render(singleCommand(wire({ exit_code: null, stdout: null, stderr: { total_bytes: 0, retained_bytes: 0, truncated: false } }), "failed"));
    const partial = lastAccepted(view.host, "marker");
    assert.equal(partial.facts.get(COMMAND_FACT.exit), COMMAND_VALUE.unknown);
    assert.equal(partial.facts.get(COMMAND_FACT.stdout), COMMAND_VALUE.unavailable);
    assert.ok(partial.facts.get(COMMAND_FACT.stderr)?.includes("0"));
    assert.notEqual(partial.facts.get(COMMAND_FACT.stderr), COMMAND_VALUE.unavailable);
  } finally { await view.close(); }
});

test("invalid timestamps, timeout and label never render Invalid Date, NaN, negative elapsed or guesses", async () => {
  const view = await mount(singleCommand(wire()));
  try {
    const cases: Array<[Partial<CommandResultMetadata>, boolean, boolean]> = [
      [{ started_at: ACCEPTED + 5000, finished_at: ACCEPTED + 1 }, true, true],
      [{ started_at: Number.NaN }, false, true],
      [{ finished_at: 9e15 }, true, false],
      [{ started_at: -5 }, false, true],
      [{ finished_at: ACCEPTED + 1.5 }, true, false],
    ];
    for (const [overrides, startValid, finishValid] of cases) {
      const node = singleCommand(wire(overrides));
      const handler = { ...node.outcomes[0].handlers[0], label: " ", timeout_ms: Number.NaN };
      await view.render({ ...node, outcomes: [{ bucket: "passed", handlers: [handler] }] });
      const { facts } = lastAccepted(view.host, "marker");
      const text = panel(view.host).textContent ?? "";
      assert.ok(!text.includes("Invalid Date") && !text.includes("NaN"));
      assert.equal(facts.get(COMMAND_FACT.duration), COMMAND_VALUE.notRecorded);
      assert.equal(facts.get(COMMAND_FACT.started) === COMMAND_VALUE.notRecorded, !startValid);
      assert.equal(facts.get(COMMAND_FACT.finished) === COMMAND_VALUE.notRecorded, !finishValid);
      assert.equal(facts.get(COMMAND_FACT.label), COMMAND_VALUE.notRecorded);
      assert.equal(facts.get(COMMAND_FACT.timeout), COMMAND_VALUE.notRecorded);
    }
    for (const timeout_ms of [0, -1, undefined]) {
      const node = singleCommand(wire());
      const bare: AutomationHandler = { id: "marker", type: "command", target: null, availability: "available", last_result: node.outcomes[0].handlers[0].last_result };
      await view.render({ ...node, outcomes: [{ bucket: "passed", handlers: [timeout_ms === undefined ? bare : { ...bare, timeout_ms }] }] });
      const { facts } = lastAccepted(view.host, "marker");
      assert.equal(facts.get(COMMAND_FACT.timeout), COMMAND_VALUE.notRecorded);
      assert.equal(facts.get(COMMAND_FACT.label), COMMAND_VALUE.notRecorded);
    }
  } finally { await view.close(); }
});

test("command text, raw output, cwd, env and free-form diagnostics never render while the label stays text", async () => {
  const injected = '<img src="x" data-label-sentinel="a">Write marker';
  const metadata = wire({ status: "nonzero", exit_code: 7 });
  const handlerResult = { ...result(metadata, "failed"), stdout: PRIVATE.stdout, stderr: PRIVATE.stderr };
  const handler = { ...commandHandler("marker", handlerResult, injected), command: PRIVATE.command, cwd: PRIVATE.cwd, env: { SECRET: PRIVATE.env } };
  const lastEntry = { ...handlerResult, handler_id: "marker", command: PRIVATE.command };
  const baseline = await mount(commandNode([], null));
  const baselineButtons = panel(baseline.host).querySelectorAll("button").length;
  await baseline.close();
  const view = await mount(commandNode([handler], { bucket: "passed", handlers: [lastEntry] }));
  try {
    const html = panel(view.host).innerHTML;
    for (const value of Object.values(PRIVATE)) assert.ok(!html.includes(value), "opaque private field is absent from text and attributes");
    assert.equal(panel(view.host).querySelector("img"), null);
    assert.equal(lastAccepted(view.host, "marker").facts.get(COMMAND_FACT.label), injected);
    assert.equal(panel(view.host).querySelectorAll("button").length, baselineButtons, "command results add no controls");
  } finally { await view.close(); }
});

test("results join their handler only within the exact bucket and keep distinct executions apart", async () => {
  const older = result(wire(), "completed", OTHER_EXECUTION);
  const newer = result(wire({ status: "nonzero", exit_code: 7 }), "failed", EXECUTION);
  const orphan = result(wire({ status: "deadline", exit_code: null }), "failed", "44444444-4444-4444-8444-444444444444");
  const node = commandNode([], { bucket: "failed", handlers: [{ ...newer, handler_id: "marker" }, { ...orphan, handler_id: "orphan" }] }, [
    { bucket: "passed", handlers: [commandHandler("marker", older, "Passed marker", 1000)] },
    { bucket: "failed", handlers: [commandHandler("marker", newer, "Failed marker", 2000)] },
  ]);
  const view = await mount(node);
  try {
    const accepted = lastAccepted(view.host, "marker");
    assert.equal(accepted.facts.get(COMMAND_FACT.label), "Failed marker");
    assert.ok(accepted.facts.get(COMMAND_FACT.timeout)?.includes("2000"));
    assert.equal(accepted.facts.get(COMMAND_FACT.outcome), "nonzero");
    assert.ok(accepted.row.textContent?.includes(EXECUTION) && !accepted.row.textContent?.includes(OTHER_EXECUTION));
    const unjoined = lastAccepted(view.host, "orphan");
    assert.equal(unjoined.facts.get(COMMAND_FACT.label), COMMAND_VALUE.notRecorded, "no label is borrowed by position or title");
    assert.equal(unjoined.facts.get(COMMAND_FACT.outcome), "deadline");
    const outcomes = commandLists(view.host, "marker").outcomes.map((list) => ({ facts: factsOf(list), row: list.closest("li")! }));
    const passed = outcomes.find((entry) => entry.row.textContent?.includes(OTHER_EXECUTION));
    assert.ok(passed, "the older execution keeps its own row");
    assert.equal(passed.facts.get(COMMAND_FACT.label), "Passed marker");
    assert.equal(passed.facts.get(COMMAND_FACT.outcome), "success");
    assert.equal(passed.facts.get(COMMAND_FACT.exit), "0");
    assert.ok(!passed.row.textContent?.includes(EXECUTION));
    const failed = outcomes.find((entry) => entry.row.textContent?.includes(EXECUTION));
    assert.ok(failed);
    assert.equal(failed.facts.get(COMMAND_FACT.outcome), undefined, "the accepted execution is shown once");
  } finally { await view.close(); }
});

test("noncommand handlers keep their existing detail without command facts", async () => {
  const node = fixtureGraph().nodes[0];
  const view = await mount(node);
  try {
    assert.equal(panel(view.host).querySelectorAll("dl[aria-label]").length, 0);
    for (const handler of node.outcomes.flatMap((outcome) => outcome.handlers)) assert.ok(panel(view.host).textContent?.includes(`${handler.id} · ${automationKindLabel(handler.type)} · ${handler.availability}`));
    for (const entry of node.last_result!.handlers) assert.ok(panel(view.host).textContent?.includes(entry.execution_id!));
  } finally { await view.close(); }
});

test("purpose, state and the latest command outcome lead while executor identity sits in a keyboard disclosure", async () => {
  const executor = { kind: "executor-kind-a", provider_id: "provider-a", adapter_id: null, model: null, capabilities: null };
  const node: AutomationNode = { ...singleCommand(wire({ status: "nonzero", exit_code: 7, resources_confirmed: false }), "failed"), executor };
  let closed = 0;
  const view = await mount(node, () => { closed++; });
  try {
    const details = panel(view.host);
    assert.equal(document.activeElement, details, "details take focus on open");
    const purpose = Array.from(details.querySelectorAll("p")).find((entry) => entry.textContent === node.purpose);
    const accepted = details.querySelector('ul[aria-label="Last accepted handler results"]');
    const toggle = details.querySelector<HTMLButtonElement>("button[aria-expanded]");
    assert.ok(purpose && accepted && toggle);
    const region = document.getElementById(toggle.getAttribute("aria-controls") ?? "");
    assert.ok(region);
    const before = (a: Node, b: Node) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
    assert.ok(before(purpose, accepted) && before(accepted, toggle), "purpose, then latest outcome, then technical disclosure");
    const order = [...lastAccepted(view.host, "marker").facts.keys()];
    for (const lead of [COMMAND_FACT.outcome, COMMAND_FACT.exit, COMMAND_FACT.resources]) {
      for (const trail of [COMMAND_FACT.timeout, COMMAND_FACT.stdout, COMMAND_FACT.started]) assert.ok(order.indexOf(lead) < order.indexOf(trail), `${lead} precedes ${trail}`);
    }
    assert.equal(lastAccepted(view.host, "marker").facts.get(COMMAND_FACT.resources), COMMAND_VALUE.unconfirmed);
    assert.ok(!region.contains(accepted) && accepted.closest("[hidden]") === null, "failure and resource uncertainty are never collapsed");
    assert.equal(toggle.type, "button");
    assert.equal(toggle.getAttribute("aria-expanded"), "false");
    assert.equal(region.hidden, true);
    for (const value of [executor.kind, executor.provider_id, node.identity.id, node.identity.owner_tab_id!]) assert.ok(region.textContent?.includes(value), "collapsed region keeps every authoritative field");
    await act(async () => { toggle.focus(); toggle.click(); });
    assert.equal(toggle.getAttribute("aria-expanded"), "true");
    assert.equal(region.hidden, false);
    await act(async () => { toggle.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert.equal(closed, 1, "Escape from inside the disclosure still closes details");
  } finally { await view.close(); }
});

let finished = false;
async function main() {
  for (const [name, run] of tests) { await run(); console.log(`✓ ${name}`); }
  console.log(`${tests.length} CommandResultDetails cases passed`);
  finished = true;
}
process.once("beforeExit", () => {
  if (finished) return;
  console.error("CommandResultDetails exited before its expected state event");
  process.exitCode = 1;
});
void main().catch((error: unknown) => {
  finished = true;
  console.error(error);
  process.exitCode = 1;
});

import assert from "node:assert/strict";
import { AutomationResource, type AutomationSnapshot } from "./resource";
import { isScenarioRoot, scopedAutomationGraph } from "./model";
import { automationKindLabel, automationKey, type AutomationGraph } from "./types";
import { attachment, cronIdentity, deferred, fixtureGraph, fixtureNode, FixtureTransport, identity, multiScenarioGraph, sentinelIdentity, workerIdentity } from "./fixtures.testdata";

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, run: () => void | Promise<void>) { tests.push([name, run]); }
test("Blueprint presentation preserves stored graph identity", () => {
  const stored = { ...sentinelIdentity };
  assert.equal(automationKindLabel(stored.kind), "node");
  assert.equal(automationKindLabel(cronIdentity.kind), "cron");
  assert.deepEqual(stored, sentinelIdentity);
});

function nextSnapshot(resource: AutomationResource, predicate: (snapshot: AutomationSnapshot) => boolean) {
  const ready = deferred<void>();
  const off = resource.subscribe(() => {
    if (!predicate(resource.getSnapshot())) return;
    off();
    ready.resolve();
  });
  return ready.promise;
}

test("exact ID and ownership joins retain missing requested identity despite duplicate names", () => {
  const graph = fixtureGraph();
  const requested = identity("worker", workerIdentity.id, "different-owner");
  const scoped = scopedAutomationGraph({ ...graph, nodes: [...graph.nodes, fixtureNode(identity("worker", "same-name"), "Worker A")], edges: [{ ...graph.edges[1], target: requested }] }, attachment);
  const missing = scoped.nodes.find((node) => automationKey(node.identity) === automationKey(requested));
  assert.ok(missing);
  assert.equal(missing.availability, "missing");
  assert.deepEqual(missing.identity, requested);
  assert.equal(missing.state, null);
  assert.equal(scoped.edges[0].evidence.kind, "configured");
});

test("foreign Team/workspace nodes and links are excluded and wrong snapshot scope refuses", () => {
  const graph = fixtureGraph();
  const foreignTeam = { ...workerIdentity, team_id: "team-foreign" };
  const foreignWorkspace = { ...cronIdentity, workspace_id: "workspace-foreign" };
  const scoped = scopedAutomationGraph({ ...graph, nodes: [...graph.nodes, fixtureNode(foreignTeam, "private-team"), fixtureNode(foreignWorkspace, "private-workspace")], edges: [...graph.edges, { ...graph.edges[0], id: "foreign-edge", target: foreignTeam }] }, attachment);
  assert.ok(!scoped.nodes.some((node) => node.name.startsWith("private-")));
  assert.ok(!scoped.edges.some((edge) => edge.id === "foreign-edge"));
  assert.throws(() => scopedAutomationGraph({ ...graph, team_id: null }, attachment));
});

test("subscription attaches before initial read and releases on last subscriber", async () => {
  const transport = new FixtureTransport();
  const resource = new AutomationResource(transport, attachment);
  const off = resource.subscribe(() => undefined);
  assert.equal(transport.reads, 0);
  await nextSnapshot(resource, (snapshot) => snapshot.graph !== null);
  assert.equal(transport.reads, 1);
  assert.deepEqual(transport.attachments, [attachment, attachment]);
  off();
  assert.equal(transport.unsubscribeCount, 1);
  assert.equal(resource.getSnapshot().graph, null);
});

test("event while reading discards obsolete record and coalesces another read", async () => {
  const transport = new FixtureTransport();
  const first = deferred<AutomationGraph>();
  transport.readOverride = async () => transport.reads === 1 ? first.promise : { ...fixtureGraph(), revision: "revision-new" };
  const resource = new AutomationResource(transport, attachment);
  const revisions: Array<string | undefined> = [];
  const off = resource.subscribe(() => { revisions.push(resource.getSnapshot().graph?.revision); });
  await transport.readStarted.promise;
  const updated = nextSnapshot(resource, (snapshot) => snapshot.graph?.revision === "revision-new");
  transport.changed!();
  transport.changed!();
  first.resolve(fixtureGraph());
  await updated;
  assert.equal(transport.reads, 2);
  assert.ok(!revisions.includes("revision-a"));
  off();
});

test("detached response and old event cannot restore stale attachment state", async () => {
  const transport = new FixtureTransport();
  const resource = new AutomationResource(transport, attachment);
  const off = resource.subscribe(() => undefined);
  await nextSnapshot(resource, (snapshot) => snapshot.graph !== null);
  const first = deferred<AutomationGraph>();
  let signal: AbortSignal | undefined;
  transport.readOverride = async (_scope, readSignal) => { signal = readSignal; return first.promise; };
  const pending = resource.refresh();
  const event = transport.changed!;
  off();
  assert.ok(signal?.aborted);
  event();
  first.resolve(fixtureGraph());
  await pending;
  assert.equal(resource.getSnapshot().graph, null);
  assert.equal(transport.reads, 2);
});

test("stop rejects unavailable capabilities and stale reads without sending operation", async () => {
  const transport = new FixtureTransport();
  const resource = new AutomationResource(transport, attachment);
  const off = resource.subscribe(() => undefined);
  await nextSnapshot(resource, (snapshot) => snapshot.graph !== null);
  await resource.stop(workerIdentity);
  await resource.stop({ ...sentinelIdentity, owner_tab_id: "wrong-owner" });
  assert.equal(transport.stops.length, 0);
  transport.readOverride = async () => { throw new Error("failure"); };
  const failed = nextSnapshot(resource, (snapshot) => snapshot.failed);
  transport.changed!();
  await failed;
  await resource.stop(sentinelIdentity);
  assert.equal(transport.stops.length, 0);
  off();
});

test("only stored agent Sentinel roots and exact connected participants enter projection", () => {
  const graph = fixtureGraph();
  const unrelatedCron = fixtureNode(identity("cron", "unrelated-cron"), "Unrelated schedule");
  const unrelatedWorker = fixtureNode(identity("worker", "unrelated-worker"), "Unrelated worker");
  const internal = fixtureNode(identity("blueprint", "internal-monitor"), "Internal monitor");
  const result = scopedAutomationGraph({ ...graph, nodes: [...graph.nodes, unrelatedCron, unrelatedWorker, internal], edges: [...graph.edges, { ...graph.edges[0], id: "internal-route", source: internal.identity, target: unrelatedWorker.identity }] }, attachment);
  assert.deepEqual(result.nodes.map((node) => node.identity.id), ["sentinel-a", "cron-a", "worker-a", "missing-a"]);
  assert.deepEqual(result.edges.map((edge) => edge.id).sort(), graph.edges.map((edge) => edge.id).sort());
});

test("scenario keys do not replace the declared root identity", () => {
  const graph = fixtureGraph();
  const root = graph.nodes[0];
  const scenario = { ...graph.scenarios[0], id: "independent-view-key" };
  const result = scopedAutomationGraph({ ...graph, scenarios: [scenario] }, attachment);
  assert.ok(isScenarioRoot(root, scenario.root));
  assert.ok(result.nodes.some((node) => node.identity.id === root.identity.id));
  for (const identity of [
    { ...root.identity, id: "other-root" },
    { ...root.identity, workspace_id: "other-workspace" },
    { ...root.identity, team_id: "other-team" },
    { ...root.identity, owner_tab_id: "other-owner" },
  ]) assert.equal(isScenarioRoot(root, identity), false);
});

test("shell-created scenario roots remain visible without agent authorship", () => {
  const graph = fixtureGraph();
  const root = graph.nodes[0];
  const result = scopedAutomationGraph({ ...graph, nodes: [{ ...root, blueprint_definition: null }, ...graph.nodes.slice(1)] }, attachment);
  assert.ok(result.nodes.some((node) => node.identity.id === root.identity.id));
  assert.equal(result.scenarios.length, graph.scenarios.length);
});

test("scenario roots preserve exact internal reachability and shared execution records", () => {
  const graph = multiScenarioGraph();
  const union = scopedAutomationGraph(graph, attachment);
  assert.equal(union.scenarios.length, 2);
  assert.equal(union.nodes.filter((node) => node.identity.id === "cron-a").length, 1);
  assert.equal(union.edges.filter((edge) => edge.id === "edge-c").length, 1);
  const first = scopedAutomationGraph({ ...graph, scenarios: [graph.scenarios[0]] }, attachment);
  assert.ok(first.nodes.some((node) => node.identity.id === "stage-a"));
  assert.ok(!first.nodes.some((node) => node.identity.id === "sentinel-b"));
  const second = scopedAutomationGraph({ ...graph, scenarios: [graph.scenarios[1]] }, attachment);
  assert.deepEqual(second.nodes.map((node) => node.identity.id), ["sentinel-b", "cron-a", "worker-a"]);
  assert.deepEqual(second.edges.find((edge) => edge.id === "edge-c"), graph.edges.find((edge) => edge.id === "edge-c"));
  const cycle = { ...graph.edges[0], id: "cycle-back", source: graph.nodes.find((node) => node.identity.id === "stage-a")!.identity, target: graph.scenarios[0].root };
  const cyclic = scopedAutomationGraph({ ...graph, scenarios: [graph.scenarios[0]], edges: [...graph.edges, cycle] }, attachment);
  assert.equal(cyclic.nodes.length, first.nodes.length);
  assert.equal(cyclic.edges.length, first.edges.length + 1);
});

let finished = false;
async function main() {
  for (const [name, run] of tests) { await run(); console.log(`✓ ${name}`); }
  console.log(`${tests.length} AutomationResource cases passed`);
  finished = true;
}
process.once("beforeExit", () => {
  if (finished) return;
  console.error("AutomationResource exited before its expected state event");
  process.exitCode = 1;
});
void main().catch((error: unknown) => {
  finished = true;
  console.error(error);
  process.exitCode = 1;
});

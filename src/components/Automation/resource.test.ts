import assert from "node:assert/strict";
import { AutomationResource } from "./resource";
import { scopedAutomationGraph } from "./model";
import { automationKey, type AutomationGraph } from "./types";
import { attachment, cronIdentity, deferred, fixtureGraph, fixtureNode, FixtureTransport, identity, sentinelIdentity, workerIdentity } from "./fixtures.testdata";

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, run: () => void | Promise<void>) { tests.push([name, run]); }
async function observed(predicate: () => boolean) {
  for (let turn = 0; !predicate() && turn < 100; turn++) await Promise.resolve();
  assert.ok(predicate(), "observable state reached within microtask failure bound");
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
  await observed(() => resource.getSnapshot().graph !== null);
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
  await observed(() => transport.reads === 1);
  transport.changed!();
  transport.changed!();
  first.resolve(fixtureGraph());
  await observed(() => resource.getSnapshot().graph?.revision === "revision-new");
  assert.equal(transport.reads, 2);
  assert.ok(!revisions.includes("revision-a"));
  off();
});

test("detached response and old event cannot restore stale attachment state", async () => {
  const transport = new FixtureTransport();
  const first = deferred<AutomationGraph>();
  let signal: AbortSignal | undefined;
  transport.readOverride = async (_scope, readSignal) => { signal = readSignal; return first.promise; };
  const resource = new AutomationResource(transport, attachment);
  const off = resource.subscribe(() => undefined);
  await observed(() => transport.reads === 1);
  const event = transport.changed!;
  off();
  assert.ok(signal?.aborted);
  event();
  first.resolve(fixtureGraph());
  await first.promise;
  assert.equal(resource.getSnapshot().graph, null);
  assert.equal(transport.reads, 1);
});

test("stop rejects unavailable capabilities and stale reads without sending operation", async () => {
  const transport = new FixtureTransport();
  const resource = new AutomationResource(transport, attachment);
  const off = resource.subscribe(() => undefined);
  await observed(() => resource.getSnapshot().graph !== null);
  await resource.stop(workerIdentity);
  await resource.stop({ ...sentinelIdentity, owner_tab_id: "wrong-owner" });
  assert.equal(transport.stops.length, 0);
  transport.readOverride = async () => { throw new Error("failure"); };
  transport.changed!();
  await observed(() => resource.getSnapshot().failed);
  await resource.stop(sentinelIdentity);
  assert.equal(transport.stops.length, 0);
  off();
});

for (const [name, run] of tests) { await run(); console.log(`✓ ${name}`); }
console.log(`${tests.length} AutomationResource cases passed`);

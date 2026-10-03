import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationGraphView } from "./AutomationGraphView";
import { attachment, fixtureGraph, fixtureNode, FixtureTransport, identity, sentinelIdentity } from "./fixtures.testdata";
import { automationKey } from "./types";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "navigator"]) {
  Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }
async function mount(transport = new FixtureTransport()) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => { root.render(<AutomationGraphView adapter={transport} attachment={attachment} />); });
  return { host, root, transport, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}
function button(host: HTMLElement, text: string) {
  const found = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent?.includes(text));
  assert.ok(found, `button ${text} rendered`);
  return found;
}
async function click(target: HTMLButtonElement) { await act(async () => { target.focus(); target.click(); }); }
function details(host: HTMLElement) {
  const panel = host.querySelector<HTMLElement>('aside[role="region"]');
  assert.ok(panel, "details rendered");
  return panel;
}

test("branched graph renders declared nodes and exact configured versus execution edges", async () => {
  const view = await mount();
  try {
    assert.equal(view.host.querySelectorAll('[data-edge-kind="configured"]').length, 3);
    assert.equal(view.host.querySelectorAll('[data-edge-kind="observed"]').length, 1);
    assert.ok(view.host.textContent?.includes(view.transport.graph.nodes[1].schedule!));
    await click(button(view.host, "Watcher A"));
    const panel = details(view.host);
    assert.ok(panel.textContent?.includes(view.transport.graph.nodes[0].purpose!));
    for (const outcome of view.transport.graph.nodes[0].outcomes) assert.ok(panel.textContent?.includes(outcome.bucket));
    assert.ok(panel.textContent?.includes("accepted-a"));
    assert.ok(panel.textContent?.includes("15000 ms"));
    assert.ok(panel.textContent?.includes("route-a"));
    assert.ok(!panel.textContent?.includes("launch-a"), "launch is attached only to its exact source");
    assert.ok(panel.textContent?.includes("handler-a · cron · available"));
    assert.ok(panel.textContent?.includes("handler-b · sentinel · unavailable"));
    assert.ok(panel.textContent?.includes("failed"));
    await click(button(panel, "Schedule A"));
    assert.ok(details(view.host).textContent?.includes("execution-a"));
  } finally { await view.close(); }
});

test("hidden Worker and missing target remain selectable without changing tab visibility", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    await click(button(details(view.host), "Worker A"));
    assert.ok(details(view.host).textContent?.includes("Hidden"));
    assert.ok(details(view.host).textContent?.includes("idle"));
    assert.ok(details(view.host).textContent?.includes(view.transport.graph.nodes[2].executor!.provider_id!));
    assert.ok(details(view.host).textContent?.includes(view.transport.graph.nodes[2].executor!.model!));
    assert.ok(details(view.host).textContent?.includes("chat_backend"));
    await click(button(view.host, "missing-a"));
    assert.ok(details(view.host).textContent?.includes("missing"));
    assert.ok(details(view.host).textContent?.includes("Unknown"));
    assert.ok(details(view.host).textContent?.includes("owner-a"));
    assert.equal(view.transport.stops.length, 0);
  } finally { await view.close(); }
});

test("authoritative event updates selected Sentinel and live hidden Worker", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    assert.ok(details(view.host).textContent?.includes("paused"));
    view.transport.graph = { ...fixtureGraph(), revision: "revision-b", nodes: fixtureGraph().nodes.map((node) => ({ ...node, state: node.identity.kind === "worker" ? "working" : "active" })) };
    await act(async () => { view.transport.changed!(); });
    assert.ok(details(view.host).textContent?.includes("active"));
    await click(button(view.host, "Worker A"));
    assert.ok(details(view.host).textContent?.includes("working"));
    assert.ok(details(view.host).textContent?.includes("Hidden"));
    assert.equal(view.transport.reads, 2);
  } finally { await view.close(); }
});

test("stop stays pending until accepted record is read and carries exact identity", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    await click(button(view.host, "Stop Sentinel"));
    assert.equal(button(view.host, "Stopping").disabled, true);
    assert.ok(details(view.host).textContent?.includes("paused"));
    assert.deepEqual(view.transport.stops, [sentinelIdentity]);
    view.transport.graph = { ...fixtureGraph(), nodes: fixtureGraph().nodes.map((node) => automationKey(node.identity) === automationKey(sentinelIdentity) ? { ...node, state: "stopped", stop: { allowed: false, reason: "reason-a" } } : node) };
    await act(async () => { view.transport.stopResult.resolve(); });
    assert.ok(details(view.host).textContent?.includes("stopped"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, true);
    assert.ok(details(view.host).textContent?.includes("reason-a"));
  } finally { await view.close(); }
});

test("stop rejection renders truthful error without raw error payload", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    await click(button(view.host, "Stop Sentinel"));
    await act(async () => { view.transport.stopResult.reject(new Error("private-payload-a")); });
    assert.ok(view.host.querySelector('[role="alert"]'));
    assert.ok(details(view.host).textContent?.includes("paused"));
    assert.ok(!view.host.textContent?.includes("private-payload-a"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, false);
  } finally { await view.close(); }
});

test("list native buttons provide keyboard selection and detail Escape restores focus", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Show list"));
    assert.ok(view.host.querySelector('ul[aria-label="Automation participants"]'));
    const selected = button(view.host, "Worker A");
    assert.equal(selected.type, "button");
    assert.ok(selected.tabIndex >= 0, "native button remains in keyboard focus order");
    await click(selected);
    assert.equal(document.activeElement, details(view.host));
    await act(async () => { details(view.host).dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.equal(document.activeElement, selected);
  } finally { await view.close(); }
});

test("failed refresh marks retained records stale and disables stop", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    view.transport.readOverride = async () => { throw new Error("secret-a"); };
    await act(async () => { view.transport.changed!(); });
    assert.ok(view.host.querySelector('[role="alert"]'));
    assert.ok(details(view.host).textContent?.includes("Stale"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, true);
    assert.ok(!view.host.textContent?.includes("secret-a"));
  } finally { await view.close(); }
});

test("Team switch removes old details and subscribes with exact attachment", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    const next = { ...attachment, team_id: "team-b", connection_id: "connection-b" };
    view.transport.graph = { ...fixtureGraph(), team_id: next.team_id, nodes: [], edges: [] };
    await act(async () => { view.root.render(<AutomationGraphView adapter={view.transport} attachment={next} />); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.ok(!view.host.textContent?.includes("Watcher A"));
    assert.equal(view.transport.unsubscribeCount, 1);
    assert.deepEqual(view.transport.attachments[view.transport.attachments.length - 1], next);
  } finally { await view.close(); }
});

test("authoritative lifecycle values and unknown Worker stay explicit with Team filtering", async () => {
  const transport = new FixtureTransport();
  const foreign = { ...fixtureGraph().nodes[2], name: "Foreign participant", identity: { ...fixtureGraph().nodes[2].identity, team_id: "foreign-team" } };
  transport.graph = { ...fixtureGraph(), nodes: [...fixtureGraph().nodes.map((node) => node.identity.kind === "worker" ? { ...node, state: null } : node), foreign] };
  const view = await mount(transport);
  try {
    assert.ok(!view.host.textContent?.includes(foreign.name));
    await click(button(view.host, "Worker A"));
    assert.ok(details(view.host).textContent?.includes("Unknown"));
    await click(button(view.host, "Watcher A"));
    for (const state of ["active", "stopped", "expired"]) {
      transport.graph = { ...transport.graph, nodes: transport.graph.nodes.map((node) => node.identity.kind === "sentinel" ? { ...node, state } : node) };
      await act(async () => { transport.changed!(); });
      assert.ok(details(view.host).textContent?.includes(state));
    }
    transport.graph = { ...transport.graph, nodes: transport.graph.nodes.map((node) => node.identity.kind === "sentinel" ? { ...node, availability: "unavailable" as const, stop: { allowed: false, reason: null } } : node) };
    await act(async () => { transport.changed!(); });
    assert.ok(details(view.host).textContent?.includes("unavailable"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, true);
  } finally { await view.close(); }
});

test("unconnected Team automation and host monitor stay absent from graph and list", async () => {
  const transport = new FixtureTransport();
  const unconnected = [fixtureNode(identity("cron", "orphan-cron"), "Orphan schedule"), fixtureNode(identity("worker", "orphan-worker"), "Orphan worker"), fixtureNode(identity("sentinel", "host-monitor"), "Host monitor")];
  transport.graph = { ...fixtureGraph(), nodes: [...fixtureGraph().nodes, ...unconnected] };
  const view = await mount(transport);
  try {
    for (const node of unconnected) assert.ok(!view.host.textContent?.includes(node.name));
    await click(button(view.host, "Show list"));
    for (const node of unconnected) assert.ok(!view.host.textContent?.includes(node.name));
    assert.equal(view.host.querySelectorAll('ul[aria-label="Automation participants"] > li').length, 4);
    await click(button(view.host, "Watcher A"));
    assert.ok(details(view.host).textContent?.includes("stored_agent_definition"));
    assert.ok(details(view.host).textContent?.includes("owner-a"));
  } finally { await view.close(); }
});

for (const [name, run] of tests) { await run(); console.log(`✓ ${name}`); }
console.log(`${tests.length} AutomationGraphView cases passed`);

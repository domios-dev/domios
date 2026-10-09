import type { AutomationAttachment, AutomationGraph, AutomationGraphAdapter, AutomationIdentity, AutomationNode } from "./types";

export const attachment: AutomationAttachment = { connection_id: "connection-a", workspace_id: "workspace-a", team_id: "team-a" };
export const identity = (kind: AutomationIdentity["kind"], id: string, owner_tab_id = "owner-a"): AutomationIdentity => ({ kind, id, owner_tab_id, workspace_id: attachment.workspace_id, team_id: attachment.team_id });
export const sentinelIdentity = identity("blueprint", "sentinel-a");
export const cronIdentity = identity("cron", "cron-a");
export const workerIdentity = identity("worker", "worker-a");
export const missingIdentity = identity("worker", "missing-a");

export function fixtureNode(identity: AutomationIdentity, name: string): AutomationNode {
  return { identity, name, purpose: `purpose-${identity.id}`, state: "active", availability: "available", hidden: false, schedule: null, cadence_ms: null, expires_at: null, outcomes: [], last_result: null, stop: { allowed: false, reason: null } };
}

export function fixtureGraph(): AutomationGraph {
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "revision-a",
    scenarios: [{ id: "scenario-a", name: "Blueprint scenario A", root: sentinelIdentity }],
    nodes: [
      { ...fixtureNode(sentinelIdentity, "Watcher A"), blueprint_definition: { source: "stored_agent_definition", definition_id: sentinelIdentity.id, creator_tab_id: sentinelIdentity.owner_tab_id! }, state: "paused", cadence_ms: 15000, expires_at: 200000, outcomes: [{ bucket: "branch-a", handlers: [
        { id: "handler-a", type: "cron", target: cronIdentity, availability: "available", last_result: { status: "success", execution_id: "handler-execution-a", accepted_at: 100000 } },
        { id: "handler-b", type: "blueprint", target: sentinelIdentity, availability: "unavailable", last_result: { status: "failed", execution_id: "handler-execution-b", accepted_at: 100000 } },
      ] }, { bucket: "branch-b", handlers: [{ id: "handler-c", type: "send", target: workerIdentity, availability: "available", last_result: null }] }], last_result: { bucket: "accepted-a", accepted_at: 100000, handlers: [{ handler_id: "handler-a", status: "success", execution_id: "handler-execution-a", accepted_at: 100000 }, { handler_id: "handler-b", status: "failed", execution_id: "handler-execution-b", accepted_at: 100000 }] }, stop: { allowed: true, reason: null } },
      { ...fixtureNode(cronIdentity, "Schedule A"), schedule: "*/7 * * * *", state: "enabled" },
      { ...fixtureNode(workerIdentity, "Worker A"), hidden: true, state: "idle", executor: { kind: "agent", provider_id: "provider-fixture-a", adapter_id: null, model: "model-fixture-a", capabilities: ["chat_backend"] } },
    ],
    edges: [
      { id: "edge-a", source: sentinelIdentity, target: cronIdentity, bucket: "branch-a", handler_id: "handler-a", handler_type: "cron", result: null, evidence: { kind: "configured", route_id: "route-a" } },
      { id: "edge-b", source: sentinelIdentity, target: workerIdentity, bucket: "branch-b", handler_id: "handler-c", handler_type: "send", result: null, evidence: { kind: "configured", route_id: "route-b" } },
      { id: "edge-c", source: cronIdentity, target: workerIdentity, bucket: "launch-a", handler_id: "handler-launch", handler_type: "launch", result: { status: "success", execution_id: "execution-a", accepted_at: 100000 }, evidence: { kind: "observed", execution_id: "execution-a" } },
      { id: "edge-d", source: sentinelIdentity, target: missingIdentity, bucket: "branch-c", handler_id: "handler-c", handler_type: "send", result: null, evidence: { kind: "configured", route_id: "route-c" } },
    ],
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

export class FixtureTransport implements AutomationGraphAdapter {
  graph = fixtureGraph();
  changed?: () => void;
  private graphListeners = new Set<() => void>();
  reads = 0;
  readStarted = deferred<void>();
  stops: AutomationIdentity[] = [];
  deletes: AutomationIdentity[] = [];
  attachments: AutomationAttachment[] = [];
  unsubscribeCount = 0;
  stopResult = deferred<void>();
  readOverride?: AutomationGraphAdapter["read"];
  async read(scope: AutomationAttachment, signal: AbortSignal) {
    this.reads++;
    this.attachments.push(scope);
    this.readStarted.resolve();
    return this.readOverride ? this.readOverride(scope, signal) : this.graph;
  }
  async subscribe(scope: AutomationAttachment, changed: () => void) {
    this.attachments.push(scope);
    this.graphListeners.add(changed);
    this.changed = () => { for (const listener of [...this.graphListeners]) listener(); };
    return () => { this.unsubscribeCount++; this.graphListeners.delete(changed); if (!this.graphListeners.size) this.changed = undefined; };
  }
  async stop(scope: AutomationAttachment, target: AutomationIdentity) {
    this.attachments.push(scope);
    this.stops.push(target);
    return this.stopResult.promise;
  }
  async delete(scope: AutomationAttachment, target: AutomationIdentity) {
    this.attachments.push(scope);
    this.deletes.push(target);
  }
}

export function multiScenarioGraph(): AutomationGraph {
  const graph = fixtureGraph();
  const stageIdentity = identity("blueprint", "stage-a");
  const secondIdentity = identity("blueprint", "sentinel-b");
  const definition = (target: AutomationIdentity) => ({ source: "stored_agent_definition" as const, definition_id: target.id, creator_tab_id: target.owner_tab_id! });
  return {
    ...graph,
    scenarios: [...graph.scenarios, { id: "scenario-b", name: "Blueprint scenario B", root: secondIdentity }],
    nodes: [...graph.nodes,
      { ...fixtureNode(stageIdentity, "Internal stage A"), blueprint_definition: definition(stageIdentity) },
      { ...fixtureNode(secondIdentity, "Watcher B"), blueprint_definition: definition(secondIdentity) },
    ],
    edges: [...graph.edges,
      { ...graph.edges[0], id: "activation-stage-a", target: stageIdentity, handler_id: "handler-stage-a", handler_type: "blueprint", evidence: { kind: "configured", route_id: "route-stage-a" } },
      { ...graph.edges[0], id: "second-shared-cron", source: secondIdentity, handler_id: "handler-shared", evidence: { kind: "configured", route_id: "route-shared" } },
    ],
  };
}

/** Three-stage chain shaped like the BLACKSTAR scenario: Blueprint root, internal Blueprint stage, Worker target. */
export function chainGraph(): AutomationGraph {
  const [root, stage] = ["chain-root", "chain-stage"].map((id) => identity("blueprint", id));
  const worker = identity("worker", "chain-worker");
  const definition = (target: AutomationIdentity) => ({ source: "stored_agent_definition" as const, definition_id: target.id, creator_tab_id: target.owner_tab_id! });
  const result = (execution_id: string) => ({ status: "completed", execution_id, accepted_at: 100000 });
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "chain-a",
    scenarios: [{ id: "scenario-chain", name: "Chain scenario", root }],
    nodes: [
      { ...fixtureNode(root, "Chain root"), blueprint_definition: definition(root) },
      { ...fixtureNode(stage, "Chain stage"), blueprint_definition: definition(stage) },
      fixtureNode(worker, "Chain worker"),
    ],
    edges: [
      { id: "chain-edge-a", source: root, target: stage, bucket: "passed-a", handler_id: "chain-handler-a", handler_type: "blueprint", result: result("chain-execution-a"), evidence: { kind: "configured", route_id: "chain-route-a" } },
      { id: "chain-edge-b", source: stage, target: worker, bucket: "passed-b", handler_id: "chain-handler-b", handler_type: "send", result: result("chain-execution-b"), evidence: { kind: "configured", route_id: "chain-route-b" } },
    ],
  };
}

/** Decision Blueprint whose declared outcomes fan out to a real stage plus target-less command, notify and stop actions. */
export function decisionGraph(): AutomationGraph {
  const [root, stage] = ["decision-root", "decision-stage"].map((id) => identity("blueprint", id));
  const worker = identity("worker", "decision-worker");
  const definition = (target: AutomationIdentity) => ({ source: "stored_agent_definition" as const, definition_id: target.id, creator_tab_id: target.owner_tab_id! });
  const accepted = { status: "completed", execution_id: "decision-exec-marker", accepted_at: 100000, command_result: { status: "success" as const, exit_code: 0, resources_confirmed: true, stdout: null, stderr: null, started_at: 100001, finished_at: 100002 } };
  const activation = { status: "completed", execution_id: "decision-exec-stage", accepted_at: 100000 };
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "decision-a",
    scenarios: [{ id: "scenario-decision", name: "Decision scenario", root }],
    nodes: [
      { ...fixtureNode(root, "Decision root"), blueprint_definition: definition(root), outcomes: [
        { bucket: "passed-d", handlers: [
          { id: "marker-d", type: "command", label: "Marker label d", timeout_ms: 1000, target: null, availability: "available", last_result: accepted },
          { id: "activate-d", type: "blueprint", target: stage, availability: "available", last_result: activation },
        ] },
        { bucket: "failed-d", handlers: [
          { id: "notify-d", type: "notify", target: null, availability: "available", last_result: null },
          { id: "stop-d", type: "stop", target: null, availability: "available", last_result: null },
        ] },
        { bucket: "skipped-d", handlers: [] },
      ], last_result: { bucket: "passed-d", accepted_at: 100000, handlers: [{ ...accepted, handler_id: "marker-d" }, { ...activation, handler_id: "activate-d" }] } },
      { ...fixtureNode(stage, "Decision stage"), blueprint_definition: definition(stage), outcomes: [
        { bucket: "passed-e", handlers: [{ id: "send-e", type: "send", target: worker, availability: "available", last_result: null }] },
        { bucket: "failed-e", handlers: [{ id: "cleanup-e", type: "command", label: "Cleanup label e", target: null, availability: "available", last_result: null }] },
      ] },
      fixtureNode(worker, "Decision worker"),
    ],
    edges: [
      { id: "decision-edge-a", source: root, target: stage, bucket: "passed-d", handler_id: "activate-d", handler_type: "blueprint", result: activation, evidence: { kind: "configured", route_id: "decision-route-a" } },
      { id: "decision-edge-b", source: stage, target: worker, bucket: "passed-e", handler_id: "send-e", handler_type: "send", result: null, evidence: { kind: "configured", route_id: "decision-route-b" } },
    ],
  };
}

/** The observed seven-card MODEL fanout: parent command + child + stop, child pending + ready stop + failed stop. */
export function modelGraph(): AutomationGraph {
  const [parent, child] = ["model-parent", "model-child"].map((id) => identity("blueprint", id));
  const definition = (target: AutomationIdentity) => ({ source: "stored_agent_definition" as const, definition_id: target.id, creator_tab_id: target.owner_tab_id! });
  const marker = { status: "completed", execution_id: "model-exec-marker", accepted_at: 100000, command_result: { status: "success" as const, exit_code: 0, resources_confirmed: true, stdout: { total_bytes: 0, retained_bytes: 0, truncated: false }, stderr: { total_bytes: 0, retained_bytes: 0, truncated: false }, started_at: 100001, finished_at: 100209 } };
  const activation = { status: "completed", execution_id: "model-exec-child", accepted_at: 100000 };
  const readyStop = { status: "completed", execution_id: "model-exec-ready-stop", accepted_at: 100500 };
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "model-a",
    scenarios: [{ id: "scenario-model", name: "Model scenario", root: parent }],
    nodes: [
      { ...fixtureNode(parent, "Model parent"), blueprint_definition: definition(parent), state: "stopped", outcomes: [
        { bucket: "ready", handlers: [
          { id: "write-model-marker", type: "command", label: "Model marker label", timeout_ms: 10000, target: null, availability: "available", last_result: marker },
          { id: "activate-model-child", type: "blueprint", target: child, availability: "available", last_result: activation },
        ] },
        { bucket: "failed", handlers: [{ id: "parent-failed-stop", type: "stop", target: null, availability: "available", last_result: null }] },
      ], last_result: { bucket: "ready", accepted_at: 100000, handlers: [{ ...marker, handler_id: "write-model-marker" }, { ...activation, handler_id: "activate-model-child" }] } },
      { ...fixtureNode(child, "Model child"), blueprint_definition: definition(child), state: "stopped", outcomes: [
        { bucket: "pending", handlers: [] },
        { bucket: "ready", handlers: [{ id: "child-ready-stop", type: "stop", target: null, availability: "available", last_result: readyStop }] },
        { bucket: "failed", handlers: [{ id: "child-failed-stop", type: "stop", target: null, availability: "available", last_result: null }] },
      ], last_result: { bucket: "ready", accepted_at: 100500, handlers: [{ ...readyStop, handler_id: "child-ready-stop" }] } },
    ],
    edges: [
      { id: "model-edge-child", source: parent, target: child, bucket: "ready", handler_id: "activate-model-child", handler_type: "blueprint", result: activation, evidence: { kind: "configured", route_id: "model-route-child" } },
    ],
  };
}

export const MODEL_CARDS = ["Model parent", "Model child", "Model marker label", "parent-failed-stop", "pending", "child-ready-stop", "child-failed-stop"] as const;

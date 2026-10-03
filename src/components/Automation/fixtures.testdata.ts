import type { AutomationAttachment, AutomationGraph, AutomationGraphAdapter, AutomationIdentity, AutomationNode } from "./types";

export const attachment: AutomationAttachment = { connection_id: "connection-a", workspace_id: "workspace-a", team_id: "team-a" };
export const identity = (kind: AutomationIdentity["kind"], id: string, owner_tab_id = "owner-a"): AutomationIdentity => ({ kind, id, owner_tab_id, workspace_id: attachment.workspace_id, team_id: attachment.team_id });
export const sentinelIdentity = identity("sentinel", "sentinel-a");
export const cronIdentity = identity("cron", "cron-a");
export const workerIdentity = identity("worker", "worker-a");
export const missingIdentity = identity("worker", "missing-a");

export function fixtureNode(identity: AutomationIdentity, name: string): AutomationNode {
  return { identity, name, purpose: `purpose-${identity.id}`, state: "active", availability: "available", hidden: false, schedule: null, cadence_ms: null, expires_at: null, outcomes: [], last_result: null, stop: { allowed: false, reason: null } };
}

export function fixtureGraph(): AutomationGraph {
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "revision-a",
    nodes: [
      { ...fixtureNode(sentinelIdentity, "Watcher A"), sentinel_definition: { source: "stored_agent_definition", definition_id: sentinelIdentity.id, creator_tab_id: sentinelIdentity.owner_tab_id! }, state: "paused", cadence_ms: 15000, expires_at: 200000, outcomes: [{ bucket: "branch-a", handlers: [
        { id: "handler-a", type: "cron", target: cronIdentity, availability: "available", last_result: { status: "success", execution_id: "handler-execution-a", accepted_at: 100000 } },
        { id: "handler-b", type: "sentinel", target: sentinelIdentity, availability: "unavailable", last_result: { status: "failed", execution_id: "handler-execution-b", accepted_at: 100000 } },
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
  reads = 0;
  readStarted = deferred<void>();
  stops: AutomationIdentity[] = [];
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
    this.changed = changed;
    return () => { this.unsubscribeCount++; this.changed = undefined; };
  }
  async stop(scope: AutomationAttachment, target: AutomationIdentity) {
    this.attachments.push(scope);
    this.stops.push(target);
    return this.stopResult.promise;
  }
}

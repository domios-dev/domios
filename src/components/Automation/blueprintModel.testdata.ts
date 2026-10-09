import { attachment, fixtureNode, identity } from "./fixtures.testdata";
import type { AutomationGraph, AutomationHandler, AutomationIdentity, AutomationNode } from "./types";

const definition = (target: AutomationIdentity) => ({ source: "stored_agent_definition" as const, definition_id: target.id, creator_tab_id: target.owner_tab_id! });
export const watchIdentity = identity("blueprint", "bp-watch");
export const followIdentity = identity("blueprint", "bp-follow");
export const workerTarget = identity("worker", "bp-worker");
export const cronTarget = identity("cron", "bp-cron");

const handler = (id: string, type: string, extra: Partial<AutomationHandler> = {}): AutomationHandler => ({ id, type, target: null, availability: "available", last_result: null, ...extra });

export const ALL_HANDLER_TYPES = ["notify", "send", "cron", "command", "blueprint", "escalate", "respond_prompt", "stop"] as const;

/** A Jev-decided probe stage with one handler of every type, chained into a terminal-watching stage. */
export function typedGraph(): AutomationGraph {
  const watch: AutomationNode = {
    ...fixtureNode(watchIdentity, "Build watch"), blueprint_definition: definition(watchIdentity), cadence_ms: 20_000, purpose: "Tell the owner when the build breaks", stop: { allowed: true, reason: null },
    source: { type: "probe" },
    detail: { definition: {
      probe: "cargo check 2>&1 | tail -n 20", command_language: "bash", decision_kind: "agent",
      question: "green: the build passed; red: compilation failed; flaky: a network error",
      buckets: [{ name: "green", pattern: null }, { name: "red", pattern: null }, { name: "flaky", pattern: null }],
      commands: [{ bucket: "red", handler_id: "rebuild", label: "rebuild", command: "cargo clean && cargo check", command_truncated: false, timeout_ms: 60_000 }],
    } },
    outcomes: [
      { bucket: "green", handlers: [] },
      { bucket: "red", handlers: [
        handler("notify-owner", "notify"),
        handler("send-fixer", "send", { target: workerTarget }),
        handler("start-cron", "cron", { target: cronTarget }),
        handler("rebuild", "command", { label: "rebuild", timeout_ms: 60_000, last_result: { status: "completed", execution_id: "x", accepted_at: 2_000 } }),
      ] },
      { bucket: "flaky", handlers: [
        handler("follow-up", "blueprint", { target: followIdentity }),
        handler("escalate", "escalate"),
        handler("answer", "respond_prompt"),
        handler("halt", "stop"),
      ] },
    ],
  };
  const follow: AutomationNode = {
    ...fixtureNode(followIdentity, "Follow terminal"), blueprint_definition: definition(followIdentity), cadence_ms: 60_000,
    source: { type: "terminal", kind: "tab", pattern: "error\\[E\\d+\\]", idle_ms: null },
    detail: { observe_target: { kind: "tab", id: workerTarget.id }, definition: { buckets: [{ name: "fixed", pattern: "Finished" }, { name: "broken", pattern: "error" }] } },
    outcomes: [{ bucket: "fixed", handlers: [handler("loop-back", "blueprint", { target: watchIdentity })] }, { bucket: "broken", handlers: [] }],
  };
  const worker: AutomationNode = { ...fixtureNode(workerTarget, "Fixer"), executor: { kind: "agent", provider_id: "claude", adapter_id: null, model: "opus", capabilities: null } };
  const cron: AutomationNode = { ...fixtureNode(cronTarget, "Nightly sweep"), schedule: "0 3 * * *" };
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "typed-a",
    configured_fallback: { adapter_id: "typesafe-jev", model: "jev-latest" },
    scenarios: [{ id: "scenario-typed", name: "Build watch", root: watchIdentity }],
    nodes: [watch, follow, worker, cron],
    edges: [],
  };
}

export function sourceStage(id: string, source: AutomationNode["source"], extra: Partial<AutomationNode> = {}): AutomationGraph {
  const stage = identity("blueprint", id);
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: id,
    scenarios: [{ id: `scenario-${id}`, name: id, root: stage }],
    nodes: [{ ...fixtureNode(stage, id), blueprint_definition: definition(stage), source, outcomes: [{ bucket: "done", handlers: [handler(`${id}-notify`, "notify")] }], ...extra }],
    edges: [],
  };
}

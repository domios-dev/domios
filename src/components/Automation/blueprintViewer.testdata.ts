import type { BlueprintClosureDiff, BlueprintDocument, BlueprintExpandedCall, BlueprintJournalNode } from "../../lib/blueprintApi";
import { pipelineModel } from "./pipelineModel";

// Wire shape from blueprint_document::calls::projection and blueprint_calls::project at 30842b8db.
export const callee: BlueprintDocument = {
  schema_version: 2, kind: "pipeline", name: "typed-helper",
  inputs: { count: { type: "integer" } },
  steps: [{ id: "return-count", code: { lang: "python", src: "def main(p): return p['value']" }, in: { type: "integer" }, out: { type: "integer" }, routes: { done: "@stop" } }],
  result: { type: { type: "integer" }, from: "return-count.out" },
};
export const call: BlueprintExpandedCall = { node: "invoke", pin: { id: "mem:helper", hash: "a".repeat(64) }, graph: callee, children: [] };
export const document: BlueprintDocument = {
  schema_version: 2, kind: "pipeline", name: "viewer-fixture", intent: "Inspect typed calls",
  trigger: { type: "event", kind: { lit: "fixture" } },
  steps: [
    { id: "probe", run: { lit: "printf 7" }, out: { type: "integer" }, routes: { done: "invoke", failed: "@stop" } },
    { id: "skipped", run: { lit: "printf unused" }, routes: { done: "@stop" } },
    { id: "invoke", call: { ref: "mem:helper", pin: call.pin, with: { count: { bind: "probe.out" } } }, in: { type: "integer" }, out: { type: "integer" }, routes: { done: "@stop", failed: "@stop" } },
  ], result: { to: "owner" },
};
export const journal: readonly BlueprintJournalNode[] = [
  { step_index: 3, parent_step_index: 2, step_name: "return-count", state: "failed", reason: "contract_violation", result_summary: "[redacted]", call_stack: [{ node: call.node, ...call.pin }] },
  { step_index: 2, parent_step_index: null, step_name: "invoke", state: "failed", reason: "contract_violation", call_stack: [{ node: call.node, ...call.pin }] },
  { step_index: 0, parent_step_index: null, step_name: "probe", state: "done", result_summary: "7", call_stack: [] },
];
export const diff: BlueprintClosureDiff = {
  root: { added: ["new-node"], removed: ["old-node"], changed: { probe: ["routes", "out", "caps", "code"], invoke: ["call"] }, document_fields: ["imports"] },
  callees: { "mem:helper": { before: call.pin, after: { ...call.pin, hash: "b".repeat(64) }, fields: { added: [], removed: [], changed: { "return-count": ["code", "in"] }, document_fields: ["inputs", "result"] } } },
};
export function viewerPipeline() {
  return pipelineModel({ id: "viewer", name: document.name, state: "running", run_id: "live", current_step: 2, last_event_at_ms: 1000,
    call_graph: document, expanded_calls: [call], nested_run: { id: "live", nodes: [{ step_index: 2, parent_step_index: null, step_name: "invoke", state: "running", call_stack: [{ node: call.node, ...call.pin }] }] },
    runs: [{ id: "past", state: "failed", closed: true, started_at_ms: 500, last_event_at_ms: 800, current_step: null, steps: [], result: { target: { kind: "owner_tab", id: "owner" }, state: "failed", reason: "contract_violation", body_ref: null }, call_graph: document, expanded_calls: [call], nested_run: { id: "past", nodes: journal } }],
  });
}

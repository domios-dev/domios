import assert from "node:assert/strict";
import { test } from "node:test";
import { blueprintProjectionView } from "./blueprintProjectionView";

test("bounded user graph payloads are narrowed without hiding unavailable calls", () => {
  const view = blueprintProjectionView({
    call_graph: { redacted: "depth_bound" },
    expanded_calls: [{ node: "invoke", pin: { id: "callee", hash: "hash" }, graph: null, children: [], unavailable: "budget_exhausted" }],
    nested_run: { id: "run", nodes: [{ step_name: "step", state: "done" }, { redacted: "depth_bound" }] },
  });
  assert.equal(view.call_graph, null);
  assert.equal(view.expanded_calls?.[0].node, "invoke");
  assert.equal(view.expanded_calls?.[0].graph, null);
  assert.equal(view.expanded_calls?.[0].unavailable, "budget_exhausted");
  assert.deepEqual(view.nested_run?.nodes, [{ step_name: "step", state: "done" }]);
});

test("a bounded document with its required structure remains usable", () => {
  const document = { schema_version: 2, kind: "watch", name: "Example", steps: [{ id: "probe" }] };
  const snapshot = { call_graph: document, id: "outer", runs: [{ id: "raw" }] };
  const projection = blueprintProjectionView(snapshot);
  assert.equal(projection.call_graph, document);
  assert.equal("runs" in projection, false);
  assert.equal("id" in projection, false);
});

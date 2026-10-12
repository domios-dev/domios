import assert from "node:assert/strict";
import { buildGlanceView } from "./glanceModel";
import type { GlanceView } from "./ViewNodeRenderer";

assert.deepEqual(buildGlanceView(null), { nodes: [] });
const configured = buildGlanceView(null, "fixture-config");
assert.equal(configured.nodes.length, 1);
assert.equal(configured.nodes[0].kind, "button");
assert.equal("action" in configured.nodes[0] && configured.nodes[0].action, "configure-with-ai");
const view: GlanceView = { title: "fixture-title", nodes: [{ kind: "text", text: "fixture-status" }] };
assert.equal(buildGlanceView(view), view);
const augmented = buildGlanceView(view, "fixture-config");
assert.equal(augmented.title, "fixture-title");
assert.deepEqual(augmented.nodes[0], view.nodes[0]);
assert.equal(augmented.nodes.length, 2);
assert.equal(view.nodes.length, 1);
console.log("PASS: absent glance, absent glance with host action, and immutable provider view");

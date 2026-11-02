import assert from "node:assert/strict";
import test from "node:test";
import { getHelpContent } from "./content";

test("agents section explains single-authority planner attachment", () => {
  const agents = getHelpContent("en").sections.find((s) => s.id === "agents");
  assert.ok(agents, "agents section missing");
  const text = agents!.blocks
    .map((b) => (b.type === "paragraph" || b.type === "tip" ? b.text : b.type === "list" ? b.items.join(" ") : ""))
    .join(" ");
  assert.ok(text.includes("Planner") && text.includes("Orchestrator"), "pair not named");
  assert.match(text, /single authority/i, "single-authority framing missing");
  assert.match(text, /subordinate planning specialist/i, "planner-specialist framing missing");
  assert.doesNotMatch(text, /equals|same level|Neither is the boss/i, "old peer-equality framing leaked");
});

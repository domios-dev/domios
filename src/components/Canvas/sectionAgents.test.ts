// Run: npx tsx src/components/Canvas/sectionAgents.test.ts

import { rollUpSectionAgents, type SectionAgentSource } from "./sectionAgents";
import type { PresenceAgent } from "./presence/identity";
import type { Task } from "./types";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const agent = (id: string): PresenceAgent => ({ id, label: id, role: null, color: "#888" });
function task(id: string, sections: string[]): Task {
  return {
    id, name: id, type: "implement", model: null, model_rationale: null, status: "pending",
    status_label: null, section_ids: sections, body: "", intent: null, assigned_tab_id: null,
    peer: null, parent_task_id: null, canvas: { x: null, y: null, collapsed: false }, milestones: [], created_at: 0,
  };
}

{
  const tasks = [task("t1", ["s1"]), task("t2", ["s1"])];
  const src = new Map<string, SectionAgentSource>([
    ["t1", { agents: [agent("a1")], live: true }],
    ["t2", { agents: [agent("a2")], live: false }],
  ]);
  const m = rollUpSectionAgents(tasks, src);
  ok("only the LIVE task's agent rolls up to the section", m.get("s1")?.length === 1 && m.get("s1")?.[0].id === "a1");
}

{
  const tasks = [task("t1", ["s1", "s2"])];
  const src = new Map<string, SectionAgentSource>([["t1", { agents: [agent("a1")], live: true }]]);
  const m = rollUpSectionAgents(tasks, src);
  ok("agent appears in both bound sections", m.get("s1")?.[0].id === "a1" && m.get("s2")?.[0].id === "a1");
}

{
  const tasks = [task("t1", ["s1"]), task("t2", ["s1"])];
  const src = new Map<string, SectionAgentSource>([
    ["t1", { agents: [agent("a1")], live: true }],
    ["t2", { agents: [agent("a1"), agent("a2")], live: true }],
  ]);
  const m = rollUpSectionAgents(tasks, src);
  ok("section dedups by agent id, counts distinct", m.get("s1")?.length === 2);
}

{
  const tasks = [task("t1", ["s1"])];
  const src = new Map<string, SectionAgentSource>([["t1", { agents: [agent("a1")], live: false }]]);
  ok("no live work → section absent from the map", !rollUpSectionAgents(tasks, src).has("s1"));
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

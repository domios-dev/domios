// Run: npx tsx src/components/Canvas/layout/lineage.test.ts

import {
  collectPlanLineageEdges,
  ghostPlanNodeId,
  layoutWorkspace,
} from "./index";
import { plan, section, task } from "./fixtures";

let passed = 0;
let failed = 0;

function ok(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("\nlineage edge direction:");
{
  const succ = plan("pl_succ", "orchA", 20, [section("s2")], [task("t2", ["s2"])], {
    lineage: [{ to: "pl_pred", rel: "follows" }],
  });
  const edges = collectPlanLineageEdges([succ]);
  ok("one lineage edge", edges.length === 1);
  ok("follows: predecessor → successor", edges[0].from === "pl_pred" && edges[0].to === "pl_succ");
  ok("via lineage", edges[0].via === "lineage");
}

console.log("\nplan.links direction:");
{
  const b = plan("pl_b", "orchA", 2, [section("sb")], [], { links: ["pl_a"] });
  const edges = collectPlanLineageEdges([b]);
  ok("links edge owner → target", edges[0].from === "pl_b" && edges[0].to === "pl_a");
}

console.log("\narchived ghost:");
{
  const archived = plan("pl_old", "orchA", 5, [section("so")], [], {
    status: "archived",
    title: "Old effort",
  });
  const current = plan("pl_new", "orchA", 30, [section("sn")], [task("tn", ["sn"])], {
    lineage: [{ to: "pl_old", rel: "supersedes" }],
  });
  const layout = layoutWorkspace([current, archived]);
  const frame = layout.frames.find((f) => f.planId === "pl_new")!;
  const ghost = frame.nodes.find((n) => n.id === ghostPlanNodeId("pl_old"));
  ok("ghost node placed", !!ghost);
  ok("ghost kind", ghost?.kind === "plan-ghost");
  const edge = frame.edges.find((e) => e.kind === "plan-plan");
  ok("plan-plan edge exists", !!edge);
  ok("edge from ghost to plan", edge?.from === ghostPlanNodeId("pl_old") && edge?.to === "pl_new");
  const planNode = frame.nodes.find((n) => n.id === "pl_new" && n.kind === "plan");
  ok("ghost sits left of plan node", !!planNode && !!ghost && ghost.box.x + ghost.box.width <= planNode.box.x);
}

console.log("\narchived linked target ghost:");
{
  const current = plan("pl_link_src", "orchA", 30, [section("ss")], [task("ts", ["ss"])], {
    links: ["pl_link_dst"],
  });
  const archived = plan("pl_link_dst", "orchA", 5, [section("sd")], [], {
    status: "archived",
    title: "Linked archive",
  });
  const layout = layoutWorkspace([current, archived]);
  const frame = layout.frames.find((f) => f.planId === "pl_link_src")!;
  const ghost = frame.nodes.find((n) => n.id === ghostPlanNodeId("pl_link_dst"));
  const planNode = frame.nodes.find((n) => n.id === "pl_link_src" && n.kind === "plan");
  const edge = frame.edges.find((e) => e.kind === "plan-plan");
  ok("target ghost node placed", !!ghost);
  ok("edge from live plan to target ghost", edge?.from === "pl_link_src" && edge?.to === ghostPlanNodeId("pl_link_dst"));
  ok("target ghost sits right of plan node", !!planNode && !!ghost && ghost.box.x >= planNode.box.x + planNode.box.width);
  ok("bounds include target ghost", !!ghost && layout.bounds.x + layout.bounds.width >= ghost.box.x + ghost.box.width);
}

console.log("\ncross-frame active lineage:");
{
  const a = plan("pl_x_a", "orchA", 40, [section("xa")], []);
  const b = plan("pl_x_b", "orchB", 50, [section("xb")], [], {
    lineage: [{ to: "pl_x_a", rel: "follows" }],
  });
  const layout = layoutWorkspace([a, b]);
  ok("cross edge emitted", (layout.crossEdges?.length ?? 0) === 1);
  const cross = layout.crossEdges![0];
  ok("cross edge direction", cross.from === "pl_x_a" && cross.to === "pl_x_b");
}

console.log("\nno lineage → no crossEdges:");
{
  const layout = layoutWorkspace([plan("pl_solo", "orchA", 1, [section("s")], [])]);
  ok("empty crossEdges", (layout.crossEdges?.length ?? 0) === 0);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

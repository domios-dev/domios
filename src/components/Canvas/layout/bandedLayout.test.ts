// Run: npx tsx src/components/Canvas/layout/bandedLayout.test.ts

import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { layoutWorkspace } from "./index";
import {
  bandedSeedPlans,
  coreSolidNodes,
  layoutFingerprint,
  overlappingPairs,
  plan,
  task,
} from "./fixtures";

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

function overlapLabel(a: { id: string; kind: string }, b: { id: string; kind: string }) {
  return `${a.kind}:${a.id} ∩ ${b.kind}:${b.id}`;
}

console.log("banded layout - determinism:");
{
  const plans = bandedSeedPlans();
  const runs = Array.from({ length: 12 }, () => layoutFingerprint(layoutWorkspace(plans)));
  const stable = runs.every((fp) => fp === runs[0]);
  ok("12 repeated runs yield identical layout fingerprint", stable);

  const shuffled = layoutFingerprint(layoutWorkspace([...plans].reverse()));
  ok("input plan order does not change packed layout", shuffled === runs[0]);

  const frame = layoutWorkspace(plans).frames.find((f) => f.planId === "pl_banded_main")!;
  const nodeOrder = frame.nodes.map((n) => `${n.kind}:${n.id}`).join("|");
  const again = layoutWorkspace(plans).frames.find((f) => f.planId === "pl_banded_main")!;
  ok("intra-frame node order is stable", again.nodes.map((n) => `${n.kind}:${n.id}`).join("|") === nodeOrder);
}

console.log("\nbanded layout - no overlap (seeded workspace):");
{
  const layout = layoutWorkspace(bandedSeedPlans());
  ok("three active plan frames", layout.frames.length === 3);

  const frameHits = overlappingPairs(layout.frames);
  ok("plan frame boxes do not overlap", frameHits.length === 0);

  const groupHits = overlappingPairs(layout.groups);
  ok("orchestrator group boxes do not overlap", groupHits.length === 0);

  for (const frame of layout.frames) {
    const core = frame.nodes.filter((n) => n.kind === "spec" || n.kind === "plan" || n.kind === "task" || n.kind === "handoff");
    const hits = overlappingPairs(core);
    ok(`${frame.planId}: core nodes (${core.length}) have zero pairwise overlap`, hits.length === 0);
    if (hits.length) {
      for (const [a, b] of hits.slice(0, 3)) console.error(`    overlap ${overlapLabel(a, b)}`);
    }
  }

  const allCore = coreSolidNodes(layout);
  const workspaceHits = overlappingPairs(allCore);
  ok(`workspace-wide core nodes (${allCore.length}) have zero overlap`, workspaceHits.length === 0);
}

console.log("\nbanded layout - column invariants on main seed:");
{
  const frame = layoutWorkspace(bandedSeedPlans()).frames.find((f) => f.planId === "pl_banded_main")!;
  const spec = frame.nodes.find((n) => n.kind === "spec")!;
  const planNode = frame.nodes.find((n) => n.kind === "plan")!;
  const tasks = frame.nodes.filter((n) => n.kind === "task");
  const planRight = planNode.box.x + planNode.box.width;

  ok("spec bound -> spec node present", !!spec);
  ok("spec left of plan", spec.box.x + spec.box.width <= planNode.box.x + 0.5);
  ok("tasks sit right of plan", tasks.every((t) => t.box.x >= planRight - 0.5));
  ok("section-row anchors match flattened sections", frame.anchors.size === 7);
}

console.log("\nbanded layout - determinism under settings clone:");
{
  const plans = bandedSeedPlans();
  const custom = { ...DEFAULT_CANVAS_SETTINGS, frameGap: DEFAULT_CANVAS_SETTINGS.frameGap + 4 };
  const a = layoutFingerprint(layoutWorkspace(plans, custom));
  const b = layoutFingerprint(layoutWorkspace(plans, custom));
  ok("custom settings produce stable layout", a === b);
}

console.log("\nbanded layout - inbox plan without spec:");
{
  const inbox = plan("inbox", "orchA", 10, [], [task("ti", [])]);
  const layout = layoutWorkspace([inbox]);
  const frame = layout.frames[0];
  const coreHits = overlappingPairs(frame.nodes.filter((n) => n.kind === "plan" || n.kind === "task"));
  ok("no spec node when unbound", !frame.nodes.some((n) => n.kind === "spec"));
  ok("plan + task do not overlap", coreHits.length === 0);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

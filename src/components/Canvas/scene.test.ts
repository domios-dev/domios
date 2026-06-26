// Run: npx tsx src/components/Canvas/scene.test.ts

import { layoutWorkspace } from "./layout";
import { bandedSeedPlans } from "./layout/fixtures";
import { buildCanvasScene, projectSceneBoxes, projectWorldBox } from "./scene";

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

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

const plans = bandedSeedPlans();
const layout = layoutWorkspace(plans);

console.log("scene builder:");
{
  const a = buildCanvasScene({ layout, plans, cameraScale: 1 });
  const b = buildCanvasScene({ layout, plans, cameraScale: 1 });
  ok("deterministic for identical layout input", sameJson(a, b));
  ok("records plan nodes", a.nodes.some((node) => node.kind === "plan" && node.id === "pl_banded_main"));
  ok("records spec nodes", a.nodes.some((node) => node.kind === "spec" && node.planId === "pl_banded_main"));
  ok("records task nodes", a.nodes.some((node) => node.kind === "task" && node.id === "t_parent"));
  ok("records element nodes", a.nodes.some((node) => node.kind === "handoff" && node.id === "ho_1"));
  ok("records edge geometry", a.edges.length > 0 && a.edges.every((edge) => edge.anchors.from && edge.anchors.to && edge.worldBox.width >= 0 && edge.lod.worldWidth >= 0));
  ok("records owner groups", a.groups.length === layout.groups.length && a.groups.every((group) => group.kind === "orchestrator-group"));
  ok("records semantic text", a.nodes.some((node) => node.id === "t_parent" && node.semanticText === "t_parent"));
  ok("records status", a.nodes.some((node) => node.id === "t_parent" && node.status === "in_progress"));
  ok("records lod inputs", a.nodes.every((node) => node.lod.worldWidth === node.worldBox.width && node.lod.thresholds.length === 3));
}

console.log("\nscale invariants:");
{
  const small = buildCanvasScene({ layout, plans, cameraScale: 0.5 });
  const large = buildCanvasScene({ layout, plans, cameraScale: 2 });
  const smallBoxes = small.nodes.map((node) => [node.id, node.worldBox]);
  const largeBoxes = large.nodes.map((node) => [node.id, node.worldBox]);
  ok("world boxes stay stable across camera scale", sameJson(smallBoxes, largeBoxes));

  const taskSmall = small.nodes.find((node) => node.id === "t_parent")!;
  const taskLarge = large.nodes.find((node) => node.id === "t_parent")!;
  ok("apparent width tracks camera scale", taskLarge.lod.apparentWidth === taskSmall.lod.apparentWidth * 4);
}

console.log("\nprojection helpers:");
{
  const scene = buildCanvasScene({ layout, plans, cameraScale: 1 });
  const node = scene.nodes.find((entry) => entry.id === "t_parent")!;
  const t1 = { x: 10, y: 20, scale: 1 };
  const t2 = { x: 10, y: 20, scale: 2 };
  const b1 = projectWorldBox(node.worldBox, t1);
  const b2 = projectWorldBox(node.worldBox, t2);
  ok("projectWorldBox maps world box to screen box", b1.x === node.worldBox.x + 10 && b1.width === node.worldBox.width);
  ok("projectWorldBox scales screen dimensions only", b2.width === node.worldBox.width * 2 && b2.height === node.worldBox.height * 2);

  const projected = projectSceneBoxes(scene, t2);
  const projectedNode = projected.find((entry) => entry.id === "t_parent")!;
  ok("projectSceneBoxes includes node screen boxes", sameJson(projectedNode.screenBox, b2));
  ok("projectSceneBoxes preserves world boxes", sameJson(projectedNode.worldBox, node.worldBox));
}

if (failed) {
  console.error(`\n${failed} failed, ${passed} passed`);
  process.exit(1);
}
console.log(`\n${passed} passed`);

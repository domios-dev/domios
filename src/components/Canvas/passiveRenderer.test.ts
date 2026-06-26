// Run: npx tsx src/components/Canvas/passiveRenderer.test.ts

import { layoutWorkspace } from "./layout";
import { bandedSeedPlans } from "./layout/fixtures";
import { buildCanvasScene, projectWorldBox } from "./scene";
import {
  passiveRingPriority,
  ringKindForNode,
  statusColorForNode,
  type PassiveRenderFocus,
  type PassiveRendererPalette,
} from "./passiveRenderer";

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

const palette: PassiveRendererPalette = {
  bg: "#111318",
  fg: "#e8eaed",
  border: "rgba(148, 163, 184, 0.45)",
  borderFocus: "rgba(129, 140, 248, 0.92)",
  groupStroke: "rgba(148, 163, 184, 0.35)",
  taskStatus: {
    pending: "rgba(148, 163, 184, 0.85)",
    in_progress: "rgba(129, 140, 248, 0.92)",
    in_review: "rgba(245, 158, 11, 0.92)",
    blocked: "rgba(239, 130, 130, 0.92)",
    done: "rgba(82, 180, 145, 0.9)",
    cancelled: "rgba(100, 116, 139, 0.9)",
  },
  planActive: "rgba(129, 140, 248, 0.92)",
  planArchived: "rgba(100, 116, 139, 0.9)",
};

const emptyFocus: PassiveRenderFocus = {
  focusedNodeId: null,
  dominantNodeId: null,
  hoveredTask: null,
  highlightTaskIds: null,
  tagFilterMatchIds: null,
  blinkIds: new Set(),
  pulsePlanIds: new Set(),
};

const plans = bandedSeedPlans();
const layout = layoutWorkspace(plans);
const scene = buildCanvasScene({ layout, plans, cameraScale: 1 });
const task = scene.nodes.find((n) => n.id === "t_parent")!;

console.log("passive renderer helpers:");
{
  ok("task status color resolves", statusColorForNode("task", "in_progress", palette) === palette.taskStatus.in_progress);
  ok("plan active color resolves", statusColorForNode("plan", "active", palette) === palette.planActive);
  ok("focus ring wins over hover", ringKindForNode("t_parent", {
    ...emptyFocus,
    hoveredTask: "t_parent",
    focusedNodeId: "t_parent",
  }) === "focus");
  ok("blink ring when not focused", ringKindForNode("t_parent", {
    ...emptyFocus,
    blinkIds: new Set(["t_parent"]),
  }) === "blink");
  ok("ring priority is stable", passiveRingPriority()[0] === "focus");
}

console.log("\nprojection alignment:");
{
  const transform = { x: 48, y: 32, scale: 1.75 };
  const screen = projectWorldBox(task.worldBox, transform);
  ok("screen x uses camera offset + scale", Math.abs(screen.x - (task.worldBox.x * transform.scale + transform.x)) < 0.001);
  ok("screen width uses camera scale", Math.abs(screen.width - task.worldBox.width * transform.scale) < 0.001);
  ok("screen height uses camera scale", Math.abs(screen.height - task.worldBox.height * transform.scale) < 0.001);
}

if (failed) {
  console.error(`\n${failed} failed, ${passed} passed`);
  process.exit(1);
}
console.log(`\n${passed} passed`);

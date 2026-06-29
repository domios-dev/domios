// Run: npx tsx src/components/Canvas/presenceOverlays.test.ts

import { largeCanvasLayout } from "./layout/largeCanvasFixtures";
import type { LaidOutNode } from "./layout";
import type { PresenceAgent } from "./presence/identity";
import { MAX_ACTIVE_PRESENCE_OVERLAYS, buildPresenceOverlays, type PresenceSource } from "./presenceOverlays";
import type { World } from "./worlds";

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

function agent(id: string): PresenceAgent {
  return { id, label: id, role: null, color: "#4f7df0" };
}

function fixture(taskCount: 100 | 500 | 1000) {
  const layout = largeCanvasLayout(taskCount);
  const nodeById = new Map<string, LaidOutNode>();
  for (const frame of layout.frames) for (const n of frame.nodes) nodeById.set(n.id, n);
  const taskIds = [...nodeById.values()].filter((n) => n.kind === "task").map((n) => n.id);
  return { layout, nodeById, taskIds };
}

const transform = { x: 40, y: 30, scale: 1.2 };

console.log("presence overlay budget:");
{
  const { nodeById, taskIds } = fixture(1000);
  const presenceByTask = new Map<string, PresenceSource>(
    taskIds.map((id) => [id, { agents: [agent(id)], live: true, label: id }]),
  );
  const visibleIds = new Set(taskIds);
  const worldByNode = new Map<string, World>(taskIds.map((id) => [id, "W1" as World]));
  const overlays = buildPresenceOverlays({ presenceByTask, nodeById, visibleIds, worldByNode, transform });
  ok("every task active → pin count capped at the budget", overlays.length === MAX_ACTIVE_PRESENCE_OVERLAYS);
  ok("budget is a hard ceiling, never exceeded", overlays.length <= MAX_ACTIVE_PRESENCE_OVERLAYS);
  ok("anchors are world-space (camera layer projects them)", overlays.every((o) => Number.isFinite(o.anchor.x) && Number.isFinite(o.anchor.y)));
  const node = nodeById.get(overlays[0]?.taskId ?? "");
  if (node) {
    const expectedX = node.box.x + node.box.width;
    ok("anchor x is task top-right in world coords", overlays[0].anchor.x === expectedX);
  }
  ok("each pin carries its agents + world band", overlays.every((o) => o.agents.length > 0 && !!o.world));
}

console.log("viewport culling:");
{
  const { nodeById, taskIds } = fixture(500);
  const presenceByTask = new Map<string, PresenceSource>(
    taskIds.map((id) => [id, { agents: [agent(id)], live: false, label: id }]),
  );
  // Only 3 tasks on-screen → only those 3 emit, regardless of how many are active.
  const onScreen = taskIds.slice(0, 3);
  const visibleIds = new Set(onScreen);
  const worldByNode = new Map<string, World>(taskIds.map((id) => [id, "W2" as World]));
  const overlays = buildPresenceOverlays({ presenceByTask, nodeById, visibleIds, worldByNode, transform });
  ok("off-screen swarm emits nothing (culled)", overlays.length === onScreen.length);
  ok("emitted pins are exactly the visible ones", overlays.every((o) => visibleIds.has(o.taskId)));
}

console.log("only-active + LOD fallback:");
{
  const { nodeById, taskIds } = fixture(100);
  // Mixed: half the visible tasks have NO agents → no pin; missing world → W0 default.
  const presenceByTask = new Map<string, PresenceSource>();
  taskIds.forEach((id, i) => {
    presenceByTask.set(id, { agents: i % 2 === 0 ? [agent(id)] : [], live: false, label: id });
  });
  const visibleIds = new Set(taskIds.slice(0, 10));
  const worldByNode = new Map<string, World>(); // empty → every pin falls back to W0
  const overlays = buildPresenceOverlays({ presenceByTask, nodeById, visibleIds, worldByNode, transform });
  ok("only tasks with agents produce a pin", overlays.every((o) => o.agents.length > 0));
  ok("idle (agentless) tasks emit no pin", overlays.length === taskIds.slice(0, 10).filter((_, i) => i % 2 === 0).length);
  ok("missing world band falls back to W0", overlays.every((o) => o.world === "W0"));
}

console.log("determinism:");
{
  const { nodeById, taskIds } = fixture(1000);
  const presenceByTask = new Map<string, PresenceSource>(
    taskIds.map((id) => [id, { agents: [agent(id)], live: true, label: id }]),
  );
  const visibleIds = new Set(taskIds);
  const worldByNode = new Map<string, World>(taskIds.map((id) => [id, "W1" as World]));
  const a = buildPresenceOverlays({ presenceByTask, nodeById, visibleIds, worldByNode, transform });
  const b = buildPresenceOverlays({ presenceByTask, nodeById, visibleIds, worldByNode, transform });
  ok("same inputs → identical selection", JSON.stringify(a.map((o) => o.taskId)) === JSON.stringify(b.map((o) => o.taskId)));
}

console.log(`\npresenceOverlays: ${passed} passed, ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);

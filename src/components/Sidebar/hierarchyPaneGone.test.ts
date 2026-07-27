// Run: npx tsx src/components/Sidebar/hierarchyPaneGone.test.ts

import type { Pane, PaneStatus } from "../../lib/types";
import { buildHierarchySubtree, buildHierarchyTree } from "./HierarchyTree";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pane(id: string, role: Pane["role"], parent?: string): Pane {
  return {
    id,
    pty_id: id,
    pane_type: "Terminal",
    title: id,
    cwd: null,
    role,
    parent_pane_id: parent ?? null,
  } as Pane;
}

function collectIds(nodes: ReturnType<typeof buildHierarchySubtree>): string[] {
  return nodes.flatMap((n) => [n.pane.id, ...collectIds(n.children)]);
}

// A worker pane assigned to an in_review task is closed on the backend. Once the
// pane snapshot drops it, the sidebar tree must not resurrect it as a live child
// even though the plan task still carries its assigned_pane_id.
const orchestrator = pane("orch", "Orchestrator");
const liveWorker = pane("worker-live", "Worker", "orch");
const goneWorker = pane("worker-gone", "Worker", "orch");
const statuses: Record<string, PaneStatus> = {
  orch: "Idle",
  "worker-live": "Working",
  "worker-gone": "Working",
};

// Before close: both workers render under the orchestrator.
{
  const panes = { orch: orchestrator, "worker-live": liveWorker, "worker-gone": goneWorker };
  const sub = buildHierarchySubtree("orch", panes, statuses);
  const ids = collectIds(sub);
  assert(ids.includes("worker-live"), "live worker present pre-close");
  assert(ids.includes("worker-gone"), "gone worker present pre-close");
}

// After close: the pane list excludes worker-gone. The tree must exclude it too,
// with no fallback that revives it from a lingering task.assigned_pane_id.
{
  const panes = { orch: orchestrator, "worker-live": liveWorker };
  const sub = buildHierarchySubtree("orch", panes, statuses);
  const ids = collectIds(sub);
  assert(ids.includes("worker-live"), "live worker still rendered");
  assert(!ids.includes("worker-gone"), "closed worker must NOT render as a live child");

  const tree = buildHierarchyTree(panes, statuses);
  const treeIds = collectIds(tree);
  assert(treeIds.includes("orch"), "orchestrator root present");
  assert(!treeIds.includes("worker-gone"), "buildHierarchyTree must not render gone worker");
}

console.log("✓ hierarchy: closed worker Tab is not rendered as a live child");

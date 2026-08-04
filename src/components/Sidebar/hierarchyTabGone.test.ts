// Run: npx tsx src/components/Sidebar/hierarchyTabGone.test.ts

import type { Tab, TabStatus } from "../../lib/types";
import { ORCHESTRATOR_KIND } from "../../lib/roleKind";
import { buildHierarchySubtree, buildHierarchyTree } from "./HierarchyTree";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function tab(id: string, role: Tab["role"], roleProfile: string | null, parent?: string): Tab {
  return {
    id,
    pty_id: id,
    tab_type: "Terminal",
    title: id,
    cwd: null,
    role,
    role_profile: roleProfile,
    parent_tab_id: parent ?? null,
  } as Tab;
}

function collectIds(nodes: ReturnType<typeof buildHierarchySubtree>): string[] {
  return nodes.flatMap((n) => [n.tab.id, ...collectIds(n.children)]);
}

// A worker tab assigned to an in_review task is closed on the backend. Once the
// tab snapshot drops it, the sidebar tree must not resurrect it as a live child
// even though the plan task still carries its assigned_tab_id.
const orchestrator = tab("orch", "Manager", ORCHESTRATOR_KIND);
const liveWorker = tab("worker-live", "Worker", null, "orch");
const goneWorker = tab("worker-gone", "Worker", null, "orch");
const statuses: Record<string, TabStatus> = {
  orch: "Idle",
  "worker-live": "Working",
  "worker-gone": "Working",
};

// Before close: both workers render under the orchestrator.
{
  const tabs = { orch: orchestrator, "worker-live": liveWorker, "worker-gone": goneWorker };
  const sub = buildHierarchySubtree("orch", tabs, statuses);
  const ids = collectIds(sub);
  assert(ids.includes("worker-live"), "live worker present pre-close");
  assert(ids.includes("worker-gone"), "gone worker present pre-close");
}

// After close: the tab list excludes worker-gone. The tree must exclude it too,
// with no fallback that revives it from a lingering task.assigned_tab_id.
{
  const tabs = { orch: orchestrator, "worker-live": liveWorker };
  const sub = buildHierarchySubtree("orch", tabs, statuses);
  const ids = collectIds(sub);
  assert(ids.includes("worker-live"), "live worker still rendered");
  assert(!ids.includes("worker-gone"), "closed worker must NOT render as a live child");

  const tree = buildHierarchyTree(tabs, statuses);
  const treeIds = collectIds(tree);
  assert(treeIds.includes("orch"), "orchestrator root present");
  assert(!treeIds.includes("worker-gone"), "buildHierarchyTree must not render gone worker");
}

console.log("✓ hierarchy: closed worker Tab is not rendered as a live child");

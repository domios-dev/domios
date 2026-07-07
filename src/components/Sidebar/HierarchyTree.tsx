import type { ReactNode } from "react";
import type { Pane, PaneStatus } from "../../lib/types";

export interface HierarchyNodeData {
  pane: Pane;
  children: HierarchyNodeData[];
  status: PaneStatus | null;
}

export function buildHierarchyTree(
  panes: Record<string, Pane>,
  statuses: Record<string, PaneStatus>,
): HierarchyNodeData[] {
  const orchestrators = Object.values(panes).filter((p) => p.role === "Orchestrator");
  function buildNode(pane: Pane): HierarchyNodeData {
    const children = Object.values(panes)
      .filter((p) => p.parent_pane_id === pane.id)
      .map(buildNode);
    return { pane, children, status: statuses[pane.id] ?? null };
  }
  return orchestrators.map(buildNode);
}

/** Children of a single pane — used under an orchestrator tab in the sidebar. */
export function buildHierarchySubtree(
  rootPaneId: string,
  panes: Record<string, Pane>,
  statuses: Record<string, PaneStatus>,
  excludePaneIds?: ReadonlySet<string>,
): HierarchyNodeData[] {
  return Object.values(panes)
    .filter((p) => p.parent_pane_id === rootPaneId)
    .filter((p) => !excludePaneIds?.has(p.id))
    .map((pane) => ({
      pane,
      status: statuses[pane.id] ?? null,
      children: buildHierarchySubtree(pane.id, panes, statuses, excludePaneIds),
    }));
}

function HierarchyTreeLevel({
  nodes,
  depth,
  renderRow,
}: {
  nodes: HierarchyNodeData[];
  depth: number;
  renderRow: (node: HierarchyNodeData, depth: number) => ReactNode;
}) {
  if (nodes.length === 0) return null;

  return (
    <div
      className={depth > 0 ? "ml-3 border-l border-[var(--ct-border)] sidebar-hierarchy-level" : "sidebar-hierarchy-level"}
      data-hierarchy-depth={depth}
    >
      {nodes.map((node) => (
        <div
          key={node.pane.id}
          data-hierarchy-row="true"
          data-hierarchy-status={node.status ?? "unknown"}
          data-hierarchy-pane-id={node.pane.id}
          data-hierarchy-working={node.status === "Working" ? "true" : "false"}
          data-hierarchy-dead={node.status === "Dead" ? "true" : "false"}
          className="sidebar-hierarchy-row-wrap"
        >
          {renderRow(node, depth)}
          {node.children.length > 0 && (
            <HierarchyTreeLevel nodes={node.children} depth={depth + 1} renderRow={renderRow} />
          )}
        </div>
      ))}
    </div>
  );
}

export function HierarchyTree({
  nodes,
  depth,
  renderRow,
}: {
  nodes: HierarchyNodeData[];
  depth: number;
  renderRow: (node: HierarchyNodeData, depth: number) => ReactNode;
}) {
  return <HierarchyTreeLevel nodes={nodes} depth={depth} renderRow={renderRow} />;
}

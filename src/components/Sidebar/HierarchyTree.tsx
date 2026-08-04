import type { ReactNode } from "react";
import type { Tab, TabStatus } from "../../lib/types";

export interface HierarchyNodeData {
  tab: Tab;
  children: HierarchyNodeData[];
  status: TabStatus | null;
}

export function buildHierarchyTree(
  tabs: Record<string, Tab>,
  statuses: Record<string, TabStatus>,
): HierarchyNodeData[] {
  const orchestrators = Object.values(tabs).filter((p) => p.role === "Orchestrator");
  function buildNode(tab: Tab): HierarchyNodeData {
    const children = Object.values(tabs)
      .filter((p) => p.parent_tab_id === tab.id)
      .map(buildNode);
    return { tab, children, status: statuses[tab.id] ?? null };
  }
  return orchestrators.map(buildNode);
}

/** Children of a single tab — used under an orchestrator tab in the sidebar. */
export function buildHierarchySubtree(
  rootTabId: string,
  tabs: Record<string, Tab>,
  statuses: Record<string, TabStatus>,
  excludeTabIds?: ReadonlySet<string>,
): HierarchyNodeData[] {
  return Object.values(tabs)
    .filter((p) => p.parent_tab_id === rootTabId)
    .filter((p) => !excludeTabIds?.has(p.id))
    .map((tab) => ({
      tab,
      status: statuses[tab.id] ?? null,
      children: buildHierarchySubtree(tab.id, tabs, statuses, excludeTabIds),
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
          key={node.tab.id}
          data-hierarchy-row="true"
          data-hierarchy-status={node.status ?? "unknown"}
          data-hierarchy-tab-id={node.tab.id}
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

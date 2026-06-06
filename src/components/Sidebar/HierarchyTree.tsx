import type { AgentRole, Pane, PaneStatus } from "../../lib/types";
import { TelegramIcon } from "../Icons";
import { StatusDot } from "./StatusDot";

export const ROLE_ICONS: Record<string, string> = {
  Orchestrator: "★", // ★
  Worker: "⚙",       // ⚙
};

export function RoleIcon({ role }: { role: AgentRole }) {
  return (
    <span className="shrink-0 text-[10px] leading-none" title={role}>
      {ROLE_ICONS[role] || "·"}
    </span>
  );
}

interface HierarchyNodeData {
  pane: Pane;
  children: HierarchyNodeData[];
  status: PaneStatus | null;
}

export function buildHierarchyTree(
  panes: Record<string, Pane>,
  statuses: Record<string, PaneStatus>,
): HierarchyNodeData[] {
  // Find orchestrators (top-level nodes)
  const orchestrators = Object.values(panes).filter(
    (p) => p.role === "Orchestrator",
  );
  function buildNode(pane: Pane): HierarchyNodeData {
    const children = Object.values(panes)
      .filter((p) => p.parent_pane_id === pane.id)
      .map(buildNode);
    return { pane, children, status: statuses[pane.id] ?? null };
  }
  return orchestrators.map(buildNode);
}

export function HierarchyTree({
  nodes,
  depth,
  onJumpToPane,
  reportCounts,
}: {
  nodes: HierarchyNodeData[];
  depth: number;
  onJumpToPane: (paneId: string) => void;
  reportCounts: Record<string, number>;
}) {
  if (nodes.length === 0) return null;
  return (
    <div className={depth > 0 ? "ml-3 border-l border-[var(--ct-border)]" : ""}>
      {nodes.map((node) => {
        const rc = reportCounts[node.pane.id] || 0;
        return (
          <div key={node.pane.id}>
            <button
              onClick={() => onJumpToPane(node.pane.id)}
              className="flex items-center gap-1.5 w-full pl-2 pr-2 py-0.5 text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors text-xs truncate"
              title={`${node.pane.title} (${node.pane.role})`}
            >
              {node.pane.role && <RoleIcon role={node.pane.role} />}
              <span className={`truncate ${node.pane.role === "Orchestrator" ? "font-semibold" : ""}`}>
                {node.pane.title}
              </span>
              <StatusDot status={node.status} animate />
              {node.pane.connected_services?.includes("telegram") && (
                <span className="text-[9px] text-[var(--ct-muted-fg)]/50">
                  <TelegramIcon />
                </span>
              )}
              {rc > 0 && (
                <span className="inline-flex items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] text-[9px] font-bold leading-none">
                  {rc}
                </span>
              )}
            </button>
            <HierarchyTree
              nodes={node.children}
              depth={depth + 1}
              onJumpToPane={onJumpToPane}
              reportCounts={reportCounts}
            />
          </div>
        );
      })}
    </div>
  );
}

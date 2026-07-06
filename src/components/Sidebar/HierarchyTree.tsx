import type { Pane, PaneStatus } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { usePaneProvider } from "../../lib/usePaneProvider";
import { TelegramIcon, TerminalIcon } from "../Icons";
import { ProviderGlyph } from "../ProviderGlyph";
import { presenceColor } from "../Canvas/presence/identity";
import {
  activeWorkForPane,
  displayTaskTitle,
  type ActiveWorkSnapshot,
} from "../Canvas/planView/activeWork";

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

/** CSS-only working-row treatments — switch via `workingVariant` for screenshot comparison. */
export type HierarchyWorkingVariant = "aurora" | "drift" | "halo";

function workingVariantClass(variant: HierarchyWorkingVariant): string {
  switch (variant) {
    case "drift":
      return "sidebar-hierarchy-working--drift";
    case "halo":
      return "sidebar-hierarchy-working--halo";
    default:
      return "sidebar-hierarchy-working--aurora";
  }
}

function HierarchyPaneIcon({
  pane,
  status,
}: {
  pane: Pane;
  status: PaneStatus | null;
}) {
  const provider = usePaneProvider(pane);
  const providerName = provider?.displayName || "Agent";
  const title =
    status === "Working"
      ? `${providerName} working...`
      : status === "Waiting"
        ? `${providerName} — waiting for input`
        : providerName;

  return (
    <span data-testid="hierarchy-pane-icon" className="shrink-0 leading-none">
      {provider ? (
        <ProviderGlyph
          iconSvg={provider.iconSvg}
          iconColor={provider.iconColor}
          iconHint={provider.iconHint}
          className="text-[10px] leading-none"
          title={title}
        />
      ) : (
        <TerminalIcon className="shrink-0 scale-90" title={title} />
      )}
    </span>
  );
}

function HierarchyTaskLine({
  status,
  taskTitle,
}: {
  status: PaneStatus;
  taskTitle: string;
}) {
  return (
    <span
      data-hierarchy-task-line="true"
      className="sidebar-hierarchy-task-line block truncate text-[10px] leading-3 text-[var(--ct-muted-fg)]"
    >
      <span className="truncate">{taskTitle}</span>
      {status === "Working" ? (
        <span
          data-hierarchy-typing="true"
          className="sidebar-hierarchy-typing-dots inline-flex items-center gap-[2px] ml-1 align-middle"
          aria-hidden
        >
          <span />
          <span />
          <span />
        </span>
      ) : (
        <span className="ml-1 italic opacity-80">waiting for input</span>
      )}
    </span>
  );
}

function HierarchyRow({
  node,
  depth,
  onJumpToPane,
  reportCounts,
  uiFontScale,
  activeWork,
  workingVariant,
  rowRef,
}: {
  node: HierarchyNodeData;
  depth: number;
  onJumpToPane: (paneId: string) => void;
  reportCounts: Record<string, number>;
  uiFontScale?: number;
  activeWork: ActiveWorkSnapshot | null;
  workingVariant: HierarchyWorkingVariant;
  rowRef?: (el: HTMLButtonElement | null) => void;
}) {
  const status = node.status;
  const isOrchestrator = node.pane.role === "Orchestrator";
  const isWorker = node.pane.role === "Worker";
  const isWorking = status === "Working";
  const isWaiting = status === "Waiting";
  const isDead = status === "Dead";
  const rowText = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const rc = reportCounts[node.pane.id] || 0;
  const boundTask = activeWork ? activeWorkForPane(activeWork, node.pane.id) : null;
  const taskTitle = boundTask ? displayTaskTitle(boundTask.task) : null;
  const showTaskLine =
    isWorker && (isWorking || isWaiting) && taskTitle != null && taskTitle.length > 0;
  const presence = presenceColor(node.pane.id);

  const rowClass = [
    "sidebar-hierarchy-row group flex w-full min-w-0 items-start gap-1.5 pl-2 pr-2 py-0.5 text-left transition-colors truncate",
    rowText,
    isWorking
      ? `sidebar-hierarchy-row--working ${workingVariantClass(workingVariant)} text-[var(--ct-fg)]`
      : isDead
        ? "sidebar-hierarchy-row--dead text-[var(--ct-muted-fg)]"
        : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]",
  ].join(" ");

  return (
    <div
      data-hierarchy-row="true"
      data-hierarchy-status={status ?? "unknown"}
      data-hierarchy-pane-id={node.pane.id}
      data-hierarchy-working={isWorking ? "true" : "false"}
      data-hierarchy-dead={isDead ? "true" : "false"}
      data-hierarchy-working-variant={isWorking ? workingVariant : undefined}
      className="sidebar-hierarchy-row-wrap"
      style={{ ["--ct-hierarchy-presence" as string]: presence }}
    >
      <button
        ref={rowRef}
        type="button"
        onClick={() => onJumpToPane(node.pane.id)}
        className={rowClass}
        title={`${node.pane.title}${taskTitle ? `\n${taskTitle}` : ""}${status ? ` (${status})` : ""}`}
      >
        <HierarchyPaneIcon pane={node.pane} status={status} />
        <span className="min-w-0 flex-1 text-left">
          <span
            className={`block truncate ${isOrchestrator ? "font-semibold" : ""} ${isDead ? "sidebar-hierarchy-dead-title" : ""}`}
          >
            {node.pane.title}
            {isDead && (
              <span data-hierarchy-dead-mark="true" className="ml-1 text-[9px] opacity-70" aria-hidden>
                ✕
              </span>
            )}
          </span>
          {showTaskLine && status && taskTitle && (
            <HierarchyTaskLine status={status} taskTitle={taskTitle} />
          )}
        </span>
        {node.pane.connected_services?.includes("telegram") && (
          <span className="shrink-0 text-[9px] text-[var(--ct-muted-fg)]/50">
            <TelegramIcon />
          </span>
        )}
        {rc > 0 && (
          <span className="inline-flex shrink-0 items-center justify-center min-w-[14px] h-3.5 px-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] text-[9px] font-bold leading-none">
            {rc}
          </span>
        )}
      </button>
      {node.children.length > 0 && (
        <HierarchyTreeLevel
          nodes={node.children}
          depth={depth + 1}
          onJumpToPane={onJumpToPane}
          reportCounts={reportCounts}
          uiFontScale={uiFontScale}
          activeWork={activeWork}
          workingVariant={workingVariant}
        />
      )}
    </div>
  );
}

function HierarchyTreeLevel({
  nodes,
  depth,
  onJumpToPane,
  reportCounts,
  uiFontScale,
  activeWork,
  workingVariant,
}: {
  nodes: HierarchyNodeData[];
  depth: number;
  onJumpToPane: (paneId: string) => void;
  reportCounts: Record<string, number>;
  uiFontScale?: number;
  activeWork: ActiveWorkSnapshot | null;
  workingVariant: HierarchyWorkingVariant;
}) {
  if (nodes.length === 0) return null;

  return (
    <div
      className={depth > 0 ? "ml-3 border-l border-[var(--ct-border)] sidebar-hierarchy-level" : "sidebar-hierarchy-level"}
      data-hierarchy-depth={depth}
    >
      {nodes.map((node) => (
        <HierarchyRow
          key={node.pane.id}
          node={node}
          depth={depth}
          onJumpToPane={onJumpToPane}
          reportCounts={reportCounts}
          uiFontScale={uiFontScale}
          activeWork={activeWork}
          workingVariant={workingVariant}
        />
      ))}
    </div>
  );
}

export function HierarchyTree({
  nodes,
  depth,
  onJumpToPane,
  reportCounts,
  uiFontScale,
  activeWork = null,
  workingVariant = "aurora",
}: {
  nodes: HierarchyNodeData[];
  depth: number;
  onJumpToPane: (paneId: string) => void;
  reportCounts: Record<string, number>;
  uiFontScale?: number;
  activeWork?: ActiveWorkSnapshot | null;
  workingVariant?: HierarchyWorkingVariant;
}) {
  return (
    <HierarchyTreeLevel
      nodes={nodes}
      depth={depth}
      onJumpToPane={onJumpToPane}
      reportCounts={reportCounts}
      uiFontScale={uiFontScale}
      activeWork={activeWork}
      workingVariant={workingVariant}
    />
  );
}

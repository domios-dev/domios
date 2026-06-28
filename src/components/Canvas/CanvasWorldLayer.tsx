import type { RefObject } from "react";
import { OrchestratorGroup, type CanvasLod } from "./cards";
import { CanvasEdges } from "./CanvasEdges";
import { CanvasSectionZones } from "./CanvasSectionZones";
import { CanvasWorldNodeList } from "./CanvasWorldNodeList";
import { CanvasWorldDecorations } from "./CanvasWorldDecorations";
import { crispRasterScale, type World } from "./worlds";
import { formatLayerTransform, type ViewportTransform } from "./useCanvasViewport";
import type { LaidOutNode, WorkspaceLayout } from "./layout";
import type { CanvasLookups } from "./canvasView/models";
import type { SectionNavArgs } from "./nodeRegistry";

type Highlight = { sectionId: string; taskIds: Set<string> } | null;

export interface CanvasWorldLayerProps {
  layerRef: RefObject<HTMLDivElement | null>;
  layout: WorkspaceLayout;
  layoutRevision: string;
  transform: ViewportTransform;
  worldW: number;
  worldH: number;
  onWorldDoubleClick: (e: React.MouseEvent) => void;
  frameIds: Set<string>;
  nodeIds: Set<string>;
  visibleNodes: LaidOutNode[];
  worldByNode: Map<string, World>;
  lod: CanvasLod;
  dominantNodeId: string | null;
  lookups: CanvasLookups;
  hoveredTask: string | null;
  highlight: Highlight;
  tagFilterActive: boolean;
  tagFilterMatchIds: Set<string> | null;
  blinkIds: ReadonlySet<string>;
  pulsePlanIds: ReadonlySet<string>;
  sectionTitleById: ReadonlyMap<string, string>;
  resolveModel: (node: LaidOutNode) => unknown;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onExpand: (nodeId: string) => void;
  onArchive?: (planId: string) => void;
  onFitNode: (nodeId: string) => void;
  onCopyLink: (id: string) => void;
  onFocusStride: (milestoneId: string) => void;
  onSectionClick: (a: SectionNavArgs) => void;
  onSectionFrame: (a: SectionNavArgs) => void;
}

// The transformed world layer: orchestrator avatars + their plan connectors, the
// section→task / cross edges (SVG), the virtualized node cards, world decorations
// (hover/highlight/blink/pulse), and the region-name pills. Extracted verbatim from
// CanvasView (Phase C iter3) — pure presentation; all state/memos stay in CanvasView.
export function CanvasWorldLayer({
  layerRef,
  layout,
  layoutRevision,
  transform,
  worldW,
  worldH,
  onWorldDoubleClick,
  frameIds,
  nodeIds,
  visibleNodes,
  worldByNode,
  lod,
  dominantNodeId,
  lookups,
  hoveredTask,
  highlight,
  tagFilterActive,
  tagFilterMatchIds,
  blinkIds,
  pulsePlanIds,
  sectionTitleById,
  resolveModel,
  onOpenPane,
  onHoverTask,
  onExpand,
  onArchive,
  onFitNode,
  onCopyLink,
  onFocusStride,
  onSectionClick,
  onSectionFrame,
}: CanvasWorldLayerProps) {
  return (
    <div
      ref={layerRef}
      data-testid="canvas-world-layer"
      data-layout-revision={layoutRevision}
      onDoubleClick={onWorldDoubleClick}
      style={{
        transform: formatLayerTransform(transform),
        transformOrigin: "0 0",
        width: worldW,
        height: worldH,
        position: "relative",
        zIndex: 2,
      }}
    >
      {layout.groups.map((group) => (
        <OrchestratorGroup
          key={group.ownerKey}
          group={group}
          onOpen={onOpenPane}
        />
      ))}
      <svg className="pointer-events-none absolute inset-0" width={worldW} height={worldH} style={{ overflow: "visible" }}>
        <defs>
          <marker
            id="ct-plan-lineage-arrow"
            markerWidth="8"
            markerHeight="8"
            refX="7"
            refY="4"
            orient="auto"
          >
            <polygon points="0,0 8,4 0,8" fill="var(--ct-border-focus)" />
          </marker>
        </defs>
        {/* Zone hull outlines removed — tasks stay section-clustered in one common
            cloud; grouping reads from card tint + the region-name labels, not drawn borders. */}
        {/* Orchestrator → plan connectors: the orchestrator floats free (avatar)
            and is LINKED to each plan it runs by a thin dashed line, not a box. */}
        {layout.groups.flatMap((group) => {
          const ax = group.box.x + 24;
          const ay = group.box.y + 34;
          return layout.frames
            .filter((f) => group.planIds.includes(f.planId) && frameIds.has(f.planId))
            .map((f) => {
              const pn = f.nodes.find((n) => n.kind === "plan");
              if (!pn) return null;
              const tx = pn.box.x + pn.box.width / 2;
              const ty = pn.box.y;
              const my = ay + (ty - ay) * 0.5;
              return (
                <path
                  key={`orch-link-${group.ownerKey}-${f.planId}`}
                  data-testid="canvas-orchestrator-link"
                  d={`M ${ax} ${ay} C ${ax} ${my}, ${tx} ${my}, ${tx} ${ty}`}
                  fill="none"
                  stroke={group.ownerPaneId ? "rgba(56,189,248,0.9)" : "var(--ct-border-default)"}
                  strokeWidth={1.5}
                  strokeDasharray="5 6"
                  opacity={0.42}
                />
              );
            })
            .filter(Boolean);
        })}
        {layout.frames.map((frame) =>
          frameIds.has(frame.planId) ? (
            <CanvasEdges
              key={frame.planId}
              edges={frame.edges}
              nodeById={lookups.nodeById}
              visibleIds={nodeIds}
              hoveredTask={hoveredTask}
              highlight={highlight}
              tagFilterActive={tagFilterActive}
              tagFilterMatchIds={tagFilterMatchIds}
            />
          ) : null,
        )}
        <CanvasEdges
          edges={layout.crossEdges ?? []}
          nodeById={lookups.nodeById}
          visibleIds={nodeIds}
          hoveredTask={hoveredTask}
          highlight={highlight}
          tagFilterActive={tagFilterActive}
          tagFilterMatchIds={tagFilterMatchIds}
        />
      </svg>
      <CanvasWorldNodeList
        nodes={visibleNodes}
        lod={lod}
        worldByNode={worldByNode}
        crispRasterScale={crispRasterScale}
        transformScale={transform.scale}
        dominantNodeId={dominantNodeId}
        resolveModel={resolveModel}
        onOpenPane={onOpenPane}
        onHoverTask={onHoverTask}
        onExpand={onExpand}
        onArchive={onArchive}
        onFitNode={onFitNode}
        onCopyLink={onCopyLink}
        onFocusStride={onFocusStride}
        onSectionClick={onSectionClick}
        onSectionFrame={onSectionFrame}
        tagFilterActive={tagFilterActive}
        tagFilterMatchIds={tagFilterMatchIds}
      />
      <CanvasWorldDecorations
        nodeById={lookups.nodeById}
        nodeIds={nodeIds}
        hoveredTask={hoveredTask}
        highlight={highlight}
        tagFilterMatchIds={tagFilterMatchIds}
        blinkIds={blinkIds}
        pulsePlanIds={pulsePlanIds}
      />
      {/* Region NAME pills sit ABOVE the cards (they were covered when drawn under)
          and at each region's centre — a quiet label naming the territory. */}
      <svg className="pointer-events-none absolute inset-0" width={worldW} height={worldH} style={{ overflow: "visible" }}>
        {layout.frames.map((frame) =>
          frameIds.has(frame.planId) ? (
            <CanvasSectionZones
              key={`zone-labels-${frame.planId}`}
              zones={frame.zones}
              world={worldByNode.get(frame.planId) ?? "W1"}
              part="label"
              titleById={sectionTitleById}
            />
          ) : null,
        )}
      </svg>
    </div>
  );
}

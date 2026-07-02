import type { Pane } from "../../lib/types";
import type { LaidOutNode } from "./layout";
import type { SectionNavArgs } from "./nodeRegistry";
import type { Plan } from "./types";
import type { TaskCardModel } from "./canvasView/models";
import { PlanDetailOverlay } from "./planView/PlanDetailOverlay";

export interface CanvasDetailOverlayProps {
  nodeId: string | null;
  nodeById: Map<string, LaidOutNode>;
  resolveModel: (node: LaidOutNode) => unknown;
  panes?: Record<string, Pane>;
  onClose: () => void;
  onOpenPane?: (paneId: string) => void;
  onCopyLink?: (id: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
}

export function CanvasDetailOverlay({
  nodeId,
  nodeById,
  resolveModel,
  panes,
  onClose,
  onOpenPane,
  onSectionClick,
  onSectionFrame,
}: CanvasDetailOverlayProps) {
  if (!nodeId) return null;
  const node = nodeById.get(nodeId);
  if (!node) return null;
  if (node.kind !== "task" && node.kind !== "plan" && node.kind !== "spec") return null;
  const model = resolveModel(node);
  if (model == null) return null;

  const kind = node.kind;
  const task = kind === "task" ? (model as TaskCardModel).task : undefined;
  const plan = kind === "plan" ? (model as Plan) : undefined;
  const spec = kind === "spec" ? (model as Plan) : undefined;

  return (
    <PlanDetailOverlay
      kind={kind}
      task={task}
      plan={plan}
      spec={spec}
      planId={node.planId}
      panes={panes}
      onClose={onClose}
      onOpenPane={onOpenPane}
      onSectionClick={onSectionClick}
      onSectionFrame={onSectionFrame}
      testIdPrefix="canvas-detail"
    />
  );
}

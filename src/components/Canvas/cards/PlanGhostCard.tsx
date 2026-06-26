import type { CSSProperties } from "react";
import { Archive, GitBranch } from "lucide-react";
import type { PlanStatus } from "../types";
import { atLeast, World } from "../worlds";
import type { CanvasCardStyleProps } from "./types";

export interface PlanGhostModel {
  planId: string;
  title: string;
  status: PlanStatus;
  sectionCount?: number;
  taskCount?: number;
}

// §lineage ghost — a fixed-footprint marker for a linked/archived plan off the live
// board. Worlds-LOD: W0 glances icon + title + kind (meaningful, never куцо); W1+
// reveals its section/task mass; W3 un-clamps the title via node-internal scroll.
const shell: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px dashed var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "color-mix(in srgb, var(--ct-bg) 72%, transparent)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  opacity: 0.82,
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
  justifyContent: "center",
  padding: "6px 10px",
  gap: 3,
};

export function PlanGhostCard({ model, style, world }: CanvasCardStyleProps & { model: PlanGhostModel; world: World }) {
  const archived = model.status === "archived";
  const showCounts = atLeast(world, World.W1);
  const sections = model.sectionCount ?? 0;
  const tasks = model.taskCount ?? 0;
  const Icon = archived ? Archive : GitBranch;
  // Fixed 48px footprint fits one title line + one meta line. The worlds-LOD
  // gradient adds the section/task mass at W1+; the full title stays in the
  // native tooltip, so nothing is silently lost at any band.

  return (
    <div
      data-testid="canvas-plan-ghost"
      data-plan-id={model.planId}
      data-archived={archived ? "true" : "false"}
      data-world={world}
      style={{ ...shell, ...style }}
      title={model.title}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
        <Icon size={12} style={{ flexShrink: 0, opacity: 0.8 }} />
        <span
          style={{
            fontSize: "var(--ct-font-size-xs)",
            fontWeight: 600,
            lineHeight: 1.25,
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {model.title}
        </span>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10, flexShrink: 0 }}>
        <span style={{ opacity: "var(--ct-text-secondary-opacity)", whiteSpace: "nowrap" }}>
          {archived ? "Archived" : "Linked"}
        </span>
        {showCounts && (sections > 0 || tasks > 0) && (
          <span
            data-testid="canvas-plan-ghost-counts"
            style={{ display: "flex", alignItems: "center", gap: 6, fontFamily: "var(--ct-font-mono)", opacity: 0.75, whiteSpace: "nowrap" }}
          >
            <span>·</span>
            {sections > 0 && <span>{sections} §</span>}
            {tasks > 0 && <span>{tasks} ◆</span>}
          </span>
        )}
      </div>
    </div>
  );
}

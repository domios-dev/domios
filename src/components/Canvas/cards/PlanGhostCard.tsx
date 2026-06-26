import type { CSSProperties } from "react";
import { Archive } from "lucide-react";
import type { PlanStatus } from "../types";
import type { CanvasCardStyleProps } from "./types";

export interface PlanGhostModel {
  planId: string;
  title: string;
  status: PlanStatus;
}

const shell: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px dashed var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "color-mix(in srgb, var(--ct-bg) 72%, transparent)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  opacity: 0.72,
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
  justifyContent: "center",
  padding: "6px 10px",
  gap: 2,
};

export function PlanGhostCard({ model, style }: CanvasCardStyleProps & { model: PlanGhostModel }) {
  return (
    <div
      data-testid="canvas-plan-ghost"
      data-plan-id={model.planId}
      data-archived={model.status === "archived" ? "true" : "false"}
      style={{ ...shell, ...style }}
      title={model.title}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
        <Archive size={12} style={{ flexShrink: 0, opacity: 0.8 }} />
        <span
          style={{
            fontSize: "var(--ct-font-size-xs)",
            fontWeight: 600,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {model.title}
        </span>
      </div>
      <span style={{ fontSize: 10, opacity: "var(--ct-text-secondary-opacity)" }}>
        {model.status === "archived" ? "Archived plan" : "Linked plan"}
      </span>
    </div>
  );
}

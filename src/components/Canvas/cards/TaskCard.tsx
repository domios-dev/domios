import { useState, type CSSProperties } from "react";
import { ExternalLink } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import type { Task, TaskStatus } from "../types";
import type { CanvasCardStyleProps } from "./types";

const cardStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  padding: "10px 12px",
  boxShadow: "var(--ct-shadow-sm)",
  display: "flex",
  flexDirection: "column",
};

function statusLabel(status: TaskStatus): string {
  return status.replace(/_/g, " ");
}

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
  onOpenPane?: (paneId: string) => void;
}

export function TaskCard({ task, style, lod = "near", onOpenPane }: TaskCardProps) {
  const [whyOpen, setWhyOpen] = useState(false);
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);

  return (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      style={{ ...cardStyle, ...style, borderLeft: `3px solid ${badgeColor}` }}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <span data-testid="canvas-task-name" style={{ fontWeight: 700, fontSize: "var(--ct-font-size-md)" }}>
              {task.name}
            </span>
            {lod !== "far" && (
              <span
                data-testid="canvas-task-status"
                style={{
                  fontSize: "0.68rem",
                  padding: "1px 8px",
                  borderRadius: 999,
                  background: badgeColor,
                  color: "var(--ct-bg)",
                  textTransform: "capitalize",
                  fontWeight: 600,
                }}
              >
                {displayStatus}
              </span>
            )}
          </div>
          {lod !== "far" && (task.type || task.model) && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 6 }}>
              {task.type && (
                <span
                  data-testid="canvas-task-type"
                  style={{
                    fontSize: "0.66rem",
                    padding: "1px 6px",
                    borderRadius: "var(--ct-radius-sm)",
                    border: "1px solid var(--ct-border-subtle)",
                    opacity: "var(--ct-text-secondary-opacity)",
                    textTransform: "capitalize",
                  }}
                >
                  {task.type}
                </span>
              )}
              {task.model && (
                <span
                  data-testid="canvas-task-model"
                  style={{
                    fontSize: "0.66rem",
                    fontFamily: "var(--ct-font-mono)",
                    padding: "1px 6px",
                    borderRadius: "var(--ct-radius-sm)",
                    background: "var(--ct-bg-selected)",
                    border: "1px solid var(--ct-border-subtle)",
                  }}
                >
                  {task.model}
                </span>
              )}
            </div>
          )}
        </div>
        {task.assigned_pane_id && (
          <button
            type="button"
            data-testid="canvas-task-pane-link"
            data-pane-id={task.assigned_pane_id}
            title={`Open pane ${task.assigned_pane_id}`}
            onClick={(e) => {
              e.stopPropagation();
              onOpenPane?.(task.assigned_pane_id!);
            }}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              flexShrink: 0,
              fontSize: "0.66rem",
              padding: "2px 7px",
              borderRadius: 999,
              border: "1px solid var(--ct-border-focus)",
              background: "color-mix(in srgb, var(--ct-border-focus) 12%, transparent)",
              color: "var(--ct-fg)",
              cursor: "pointer",
            }}
          >
            <ExternalLink size={11} />
            pane
          </button>
        )}
      </div>

      {lod === "near" && task.intent && (
        <div
          data-testid="canvas-task-intent"
          style={{ marginTop: 7, fontSize: "var(--ct-font-size-sm)", fontStyle: "italic", opacity: "var(--ct-text-secondary-opacity)" }}
        >
          {task.intent}
        </div>
      )}

      {lod === "near" && task.model_rationale && (
        <div style={{ marginTop: 6 }}>
          <button
            type="button"
            data-testid="canvas-task-why-toggle"
            onClick={() => setWhyOpen((open) => !open)}
            style={{
              fontSize: "var(--ct-font-size-sm)",
              color: "var(--ct-border-focus)",
              background: "none",
              border: "none",
              padding: 0,
              cursor: "pointer",
            }}
          >
            {whyOpen ? "Hide why" : "Why this model?"}
          </button>
          {whyOpen && (
            <div
              data-testid="canvas-task-why"
              style={{ marginTop: 4, fontSize: "var(--ct-font-size-sm)", opacity: "var(--ct-text-secondary-opacity)" }}
            >
              {task.model_rationale}
            </div>
          )}
        </div>
      )}

      {lod === "near" && task.body.trim().length > 0 && (
        <div data-testid="canvas-task-body" style={{ marginTop: 8, fontSize: "var(--ct-font-size-sm)", lineHeight: 1.5 }}>
          <RichMarkdown text={task.body} />
        </div>
      )}
    </div>
  );
}

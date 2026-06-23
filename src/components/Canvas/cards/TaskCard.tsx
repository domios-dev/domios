import { useState, type CSSProperties } from "react";
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
};

function statusLabel(status: TaskStatus): string {
  return status.replace(/_/g, " ");
}

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
}

export function TaskCard({ task, style, lod = "near" }: TaskCardProps) {
  const [whyOpen, setWhyOpen] = useState(false);
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);

  return (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      style={{ ...cardStyle, ...style }}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        <span
          data-testid="canvas-task-marker"
          aria-hidden
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            marginTop: 6,
            flexShrink: 0,
            background: badgeColor,
          }}
        />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <span
              data-testid="canvas-task-name"
              style={{ fontWeight: 600, fontSize: "var(--ct-font-size-md)" }}
            >
              {task.name}
            </span>
            {lod !== "far" && task.model && (
              <span
                data-testid="canvas-task-model"
                style={{
                  fontSize: "var(--ct-font-size-sm)",
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
            {lod !== "far" && (
              <span
                data-testid="canvas-task-status"
                style={{
                  fontSize: "var(--ct-font-size-sm)",
                  padding: "1px 8px",
                  borderRadius: "var(--ct-radius-sm)",
                  background: badgeColor,
                  color: "var(--ct-bg)",
                  textTransform: "capitalize",
                }}
              >
                {displayStatus}
              </span>
            )}
          </div>
          {lod === "near" && task.model_rationale && (
            <div style={{ marginTop: 6 }}>
              <button
                type="button"
                data-testid="canvas-task-why-toggle"
                onClick={() => setWhyOpen((open) => !open)}
                style={{
                  fontSize: "var(--ct-font-size-sm)",
                  color: "var(--ct-fg)",
                  opacity: "var(--ct-text-secondary-opacity)",
                  background: "none",
                  border: "none",
                  padding: 0,
                  cursor: "pointer",
                  textDecoration: "underline",
                }}
              >
                {whyOpen ? "Hide why" : "Why this model?"}
              </button>
              {whyOpen && (
                <div
                  data-testid="canvas-task-why"
                  style={{
                    marginTop: 4,
                    fontSize: "var(--ct-font-size-sm)",
                    opacity: "var(--ct-text-secondary-opacity)",
                  }}
                >
                  {task.model_rationale}
                </div>
              )}
            </div>
          )}
          {lod === "near" && task.body.trim().length > 0 && (
            <div data-testid="canvas-task-body" style={{ marginTop: 8, fontSize: "var(--ct-font-size-sm)" }}>
              <RichMarkdown text={task.body} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

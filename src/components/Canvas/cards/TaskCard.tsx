import { useState, type CSSProperties } from "react";
import { ExternalLink } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import type { Task, TaskStatus } from "../types";
import type { CanvasCardStyleProps } from "./types";

// All content is ALWAYS mounted (no subtraction LOD → no empty boxes). Text sizes
// are `em` relative to the counter-scaled .ct-canvas-node root (semantic zoom).
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
  childCount?: number;
  onOpenPane?: (paneId: string) => void;
  onHover?: (taskId: string | null) => void;
}

export function TaskCard({ task, childCount = 0, style, onOpenPane, onHover }: TaskCardProps) {
  const [whyOpen, setWhyOpen] = useState(false);
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);

  return (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      className="ct-canvas-node"
      onMouseEnter={() => onHover?.(task.id)}
      onMouseLeave={() => onHover?.(null)}
      style={{ ...cardStyle, ...style }}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        <span
          data-testid="canvas-task-marker"
          aria-hidden
          style={{
            width: "0.7em",
            height: "0.7em",
            borderRadius: "50%",
            marginTop: "0.4em",
            flexShrink: 0,
            background: badgeColor,
            boxShadow: `0 0 0 0.22em color-mix(in srgb, ${badgeColor} 22%, transparent)`,
          }}
        />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <span data-testid="canvas-task-name" style={{ fontWeight: 700, fontSize: "1.05em" }}>
              {task.name}
            </span>
            {childCount > 0 && (
              <span
                data-testid="canvas-task-subtask-count"
                style={{
                  fontSize: "0.74em",
                  fontWeight: 600,
                  padding: "0 0.5em",
                  lineHeight: 1.7,
                  borderRadius: 999,
                  background: "color-mix(in srgb, var(--ct-border-focus) 14%, transparent)",
                  border: "1px solid color-mix(in srgb, var(--ct-border-focus) 45%, transparent)",
                  color: "var(--ct-border-focus)",
                }}
              >
                {childCount} subtask{childCount === 1 ? "" : "s"}
              </span>
            )}
            <span
              data-testid="canvas-task-status"
              style={{
                fontSize: "0.74em",
                padding: "0 0.5em",
                lineHeight: 1.7,
                borderRadius: 999,
                background: `color-mix(in srgb, ${badgeColor} 14%, transparent)`,
                border: `1px solid color-mix(in srgb, ${badgeColor} 48%, transparent)`,
                color: badgeColor,
                textTransform: "capitalize",
                fontWeight: 600,
              }}
            >
              {displayStatus}
            </span>
          </div>
          {(task.type || task.model) && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 6 }}>
              {task.type && (
                <span
                  data-testid="canvas-task-type"
                  style={{
                    fontSize: "0.78em",
                    padding: "0.08em 0.45em",
                    borderRadius: "var(--ct-radius-sm)",
                    border: "1px solid var(--ct-border-subtle)",
                    background: "var(--ct-bg-active)",
                    color: "var(--ct-fg)",
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
                    fontSize: "0.78em",
                    fontFamily: "var(--ct-font-mono)",
                    padding: "0.08em 0.45em",
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
              fontSize: "0.78em",
              padding: "0.15em 0.55em",
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

      {task.intent && (
        <div
          data-testid="canvas-task-intent"
          style={{ marginTop: 7, fontSize: "0.92em", fontStyle: "italic", opacity: "var(--ct-text-secondary-opacity)" }}
        >
          {task.intent}
        </div>
      )}

      {task.model_rationale && (
        <div style={{ marginTop: 6 }}>
          <button
            type="button"
            data-testid="canvas-task-why-toggle"
            onClick={() => setWhyOpen((open) => !open)}
            style={{
              fontSize: "0.92em",
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
              style={{ marginTop: 4, fontSize: "0.92em", opacity: "var(--ct-text-secondary-opacity)" }}
            >
              {task.model_rationale}
            </div>
          )}
        </div>
      )}

      {task.body.trim().length > 0 && (
        <div data-testid="canvas-task-body" style={{ marginTop: 8, fontSize: "0.92em", lineHeight: 1.5 }}>
          <RichMarkdown text={task.body} />
        </div>
      )}
    </div>
  );
}

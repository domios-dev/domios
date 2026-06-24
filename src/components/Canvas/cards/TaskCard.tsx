import { useState, type CSSProperties } from "react";
import { ExternalLink } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import type { Task, TaskStatus } from "../types";
import type { CanvasCardStyleProps, CanvasViewMode } from "./types";
import { digestMarkdown } from "./digest";

// Three discrete view modes (§18) — text fixed-size in each, no continuous scaling.
const cardStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "var(--ct-shadow-sm)",
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
};

function statusLabel(status: TaskStatus): string {
  return status.replace(/_/g, " ");
}

function StatusDot({ color, size = "0.7em" }: { color: string; size?: string }) {
  return (
    <span
      data-testid="canvas-task-marker"
      aria-hidden
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        flexShrink: 0,
        background: color,
        boxShadow: `0 0 0 0.22em color-mix(in srgb, ${color} 22%, transparent)`,
      }}
    />
  );
}

function StatusBadge({ color, label }: { color: string; label: string }) {
  return (
    <span
      data-testid="canvas-task-status"
      style={{
        fontSize: "0.74em",
        padding: "0 0.5em",
        lineHeight: 1.7,
        borderRadius: 999,
        background: `color-mix(in srgb, ${color} 14%, transparent)`,
        border: `1px solid color-mix(in srgb, ${color} 48%, transparent)`,
        color,
        textTransform: "capitalize",
        fontWeight: 600,
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </span>
  );
}

function SubtaskBadge({ count }: { count: number }) {
  return (
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
        whiteSpace: "nowrap",
      }}
    >
      {count}
    </span>
  );
}

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
  childCount?: number;
  onOpenPane?: (paneId: string) => void;
  onHover?: (taskId: string | null) => void;
}

export function TaskCard({
  task,
  childCount = 0,
  style,
  viewMode = "document",
  onOpenPane,
  onHover,
}: TaskCardProps) {
  const [whyOpen, setWhyOpen] = useState(false);
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);

  const wrap = (children: React.ReactNode, mode: CanvasViewMode) => (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      data-view-mode={mode}
      className="ct-canvas-node"
      onMouseEnter={() => onHover?.(task.id)}
      onMouseLeave={() => onHover?.(null)}
      style={{ ...cardStyle, ...style }}
    >
      {children}
    </div>
  );

  if (viewMode === "tile") {
    return wrap(
      <div style={{ flex: 1, display: "flex", alignItems: "center", gap: 8, padding: "0 12px", minWidth: 0 }}>
        <StatusDot color={badgeColor} size="0.85em" />
        <span
          data-testid="canvas-task-name"
          style={{ flex: 1, minWidth: 0, fontWeight: 700, fontSize: "1.02em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
        >
          {task.name}
        </span>
        {childCount > 0 && <SubtaskBadge count={childCount} />}
      </div>,
      "tile",
    );
  }

  if (viewMode === "card") {
    const digest = digestMarkdown(task.intent?.trim() ? task.intent : task.body, 150);
    return wrap(
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6, padding: "10px 12px", minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <StatusDot color={badgeColor} />
          <span
            data-testid="canvas-task-name"
            style={{ flex: 1, minWidth: 0, fontWeight: 700, fontSize: "1.05em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
          >
            {task.name}
          </span>
          {childCount > 0 && <SubtaskBadge count={childCount} />}
          <StatusBadge color={badgeColor} label={displayStatus} />
        </div>
        {digest && (
          <div
            data-testid="canvas-task-digest"
            style={{
              fontSize: "0.92em",
              lineHeight: 1.45,
              opacity: "var(--ct-text-secondary-opacity)",
              display: "-webkit-box",
              WebkitLineClamp: 2,
              WebkitBoxOrient: "vertical",
              overflow: "hidden",
            }}
          >
            {digest}
          </div>
        )}
      </div>,
      "card",
    );
  }

  return wrap(
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: "10px 12px", minWidth: 0, overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        <span style={{ marginTop: "0.4em" }}>
          <StatusDot color={badgeColor} />
        </span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <span data-testid="canvas-task-name" style={{ fontWeight: 700, fontSize: "1.05em" }}>
              {task.name}
            </span>
            {childCount > 0 && <SubtaskBadge count={childCount} />}
            <StatusBadge color={badgeColor} label={displayStatus} />
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
        <div data-testid="canvas-task-body" className="ct-canvas-scroll" style={{ marginTop: 8, fontSize: "0.92em", lineHeight: 1.5, flex: 1, overflowY: "auto", minHeight: 0 }}>
          <RichMarkdown text={task.body} />
        </div>
      )}
    </div>,
    "document",
  );
}

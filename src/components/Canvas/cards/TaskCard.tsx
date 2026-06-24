import type { CSSProperties } from "react";
import { ExternalLink, Layers } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import { atLeast, type World } from "../worlds";
import type { Task, TaskStatus } from "../types";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";
import { TaskSteps } from "./TaskSteps";

// §26 worlds task cell — fixed footprint, content adapts to the node's appW band
// (W0 mass → W3 deep). No manual mode, no click-to-reveal: zoom IS the navigation.
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

// §24.5/§25.6 status emphasis — multiplies at every band: in_progress pops with an
// accent ring (pulses at W0), done gently dims (restores at W3), blocked warns.
function statusChrome(status: TaskStatus, world: World): { ring?: string; pulse: boolean; dim: number; warnBorder: boolean } {
  if (status === "in_progress") {
    return { ring: "0 0 0 1.5px color-mix(in srgb, var(--ct-border-focus) 80%, transparent), 0 6px 22px -8px color-mix(in srgb, var(--ct-border-focus) 55%, transparent)", pulse: !atLeast(world, "W1"), dim: 1, warnBorder: false };
  }
  if (status === "done" || status === "cancelled") {
    return { pulse: false, dim: atLeast(world, "W3") ? 0.86 : 0.7, warnBorder: false };
  }
  if (status === "blocked") return { pulse: false, dim: 1, warnBorder: true };
  return { pulse: false, dim: 1, warnBorder: false };
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

// §25.5 subtask volume cue — a card-deck silhouette that reads as physical mass at
// far zoom (0-subtask tasks stay flat so heavy tasks pop). ⊞N + done/total mini-bar.
function SubtaskVolume({ count, done }: { count: number; done: number }) {
  if (count <= 0) return null;
  const layers = Math.min(3, count);
  const frac = count > 0 ? done / count : 0;
  return (
    <div data-testid="canvas-task-subtask-volume" style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <span aria-hidden style={{ position: "relative", width: 18 + layers * 3, height: 16, flexShrink: 0 }}>
        {Array.from({ length: layers }).map((_, i) => (
          <span
            key={i}
            style={{
              position: "absolute",
              left: i * 3,
              top: i * 3,
              width: 16,
              height: 12,
              borderRadius: 3,
              border: "1px solid color-mix(in srgb, var(--ct-border-focus) 55%, transparent)",
              background: "color-mix(in srgb, var(--ct-border-focus) 10%, var(--ct-bg))",
            }}
          />
        ))}
      </span>
      <span data-testid="canvas-task-subtask-count" style={{ display: "inline-flex", alignItems: "center", gap: 3, fontSize: "0.74em", fontWeight: 700, color: "var(--ct-border-focus)" }}>
        <Layers size={11} />⊞{count}
      </span>
      <span style={{ flex: 1, minWidth: 24, height: 5, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
        <span style={{ display: "block", width: `${Math.round(frac * 100)}%`, height: "100%", background: "rgba(82,180,145,0.9)" }} />
      </span>
      <span style={{ fontSize: "0.66em", fontFamily: "var(--ct-font-mono)", opacity: "var(--ct-text-secondary-opacity)" }}>{done}/{count}</span>
    </div>
  );
}

function Chip({ testid, mono, children }: { testid: string; mono?: boolean; children: React.ReactNode }) {
  return (
    <span
      data-testid={testid}
      style={{
        fontSize: "0.78em",
        fontFamily: mono ? "var(--ct-font-mono)" : undefined,
        padding: "0.08em 0.45em",
        borderRadius: "var(--ct-radius-sm)",
        border: "1px solid var(--ct-border-subtle)",
        background: mono ? "var(--ct-bg-selected)" : "var(--ct-bg-active)",
        color: "var(--ct-fg)",
        textTransform: mono ? "none" : "capitalize",
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
  childCount?: number;
  childDone?: number;
  stepColors?: Map<string, string>;
  world: World;
  onOpenPane?: (paneId: string) => void;
  onHover?: (taskId: string | null) => void;
}

export function TaskCard({
  task,
  childCount = 0,
  childDone = 0,
  stepColors,
  style,
  world,
  onOpenPane,
  onHover,
}: TaskCardProps) {
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);
  const chrome = statusChrome(task.status, world);
  const showBody = atLeast(world, "W2");
  const showStructure = atLeast(world, "W1");

  const mergedStyle: CSSProperties = {
    ...cardStyle,
    ...style,
    ...(chrome.ring ? { boxShadow: chrome.ring } : null),
    ...(chrome.warnBorder ? { borderLeft: "3px solid var(--ct-error, #ef8282)" } : null),
  };

  const paneLink = task.assigned_pane_id && showBody && (
    <button
      type="button"
      data-testid="canvas-task-pane-link"
      data-pane-id={task.assigned_pane_id}
      title={`Open pane ${task.assigned_pane_id}`}
      onClick={(e) => {
        e.stopPropagation();
        onOpenPane?.(task.assigned_pane_id!);
      }}
      onMouseDown={(e) => e.stopPropagation()}
      style={{ display: "flex", alignItems: "center", gap: 4, flexShrink: 0, fontSize: "0.78em", padding: "0.15em 0.55em", borderRadius: 999, border: "1px solid var(--ct-border-focus)", background: "color-mix(in srgb, var(--ct-border-focus) 12%, transparent)", color: "var(--ct-fg)", cursor: "pointer" }}
    >
      <ExternalLink size={11} />
      pane
    </button>
  );

  return (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      data-world={world}
      className={`ct-canvas-node${chrome.pulse ? " ct-canvas-live" : ""}`}
      onMouseEnter={() => onHover?.(task.id)}
      onMouseLeave={() => onHover?.(null)}
      style={mergedStyle}
    >
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 7, padding: "10px 12px", minWidth: 0, overflow: "hidden", opacity: chrome.dim }}>
        {/* Header — present at every band (the navigation layer). */}
        <div style={{ display: "flex", alignItems: "flex-start", gap: 8, minWidth: 0 }}>
          <span style={{ marginTop: "0.3em" }}>
            <StatusDot color={badgeColor} />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: showStructure ? "wrap" : "nowrap", minWidth: 0 }}>
              <span data-testid="canvas-task-name" style={{ flex: showStructure ? "1 0 auto" : 1, minWidth: 0, fontWeight: 700, fontSize: "1.03em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {task.name}
              </span>
              <StatusBadge color={badgeColor} label={displayStatus} />
            </div>
            {showStructure && (task.type || task.model) && (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 6 }}>
                {task.type && <Chip testid="canvas-task-type">{task.type}</Chip>}
                {task.model && <Chip testid="canvas-task-model" mono>{task.model}</Chip>}
              </div>
            )}
          </div>
          {paneLink}
        </div>

        {/* W1 digest (1-line) so structure band isn't empty above the thread. */}
        {showStructure && !showBody && (task.intent?.trim() || task.body.trim()) && (
          <div data-testid="canvas-task-digest" style={{ fontSize: "0.86em", lineHeight: 1.4, opacity: "var(--ct-text-secondary-opacity)", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
            {digestMarkdown(task.intent?.trim() ? task.intent : task.body, 120)}
          </div>
        )}

        {/* W2+ intent + model rationale revealed by ZOOM (not a click). */}
        {showBody && task.intent && (
          <div data-testid="canvas-task-intent" style={{ fontSize: "0.9em", fontStyle: "italic", opacity: "var(--ct-text-secondary-opacity)" }}>
            {task.intent}
          </div>
        )}
        {showBody && task.model_rationale && (
          <div data-testid="canvas-task-why" style={{ fontSize: "0.88em", opacity: "var(--ct-text-secondary-opacity)", borderLeft: "2px solid var(--ct-border-subtle)", paddingLeft: 8 }}>
            <span style={{ fontWeight: 600, color: "var(--ct-border-focus)" }}>Why this model? </span>
            {task.model_rationale}
          </div>
        )}

        {/* Subtask volume cue: deck at W0/W1, real child cards appear (separate
            nodes) at W2+, so here the cue fades to just a compact count. */}
        {childCount > 0 && (
          showBody
            ? <div data-testid="canvas-task-subtask-count" style={{ fontSize: "0.74em", fontWeight: 600, opacity: "var(--ct-text-secondary-opacity)" }}>⊞ {childCount} subtask{childCount === 1 ? "" : "s"}</div>
            : <SubtaskVolume count={childCount} done={childDone} />
        )}

        {/* Body + steps. W0 = constellation only (marginTop:auto pins it). W2+ = body
            (clamped at W2, full+scroll at W3) then step cards, scrolling node-only. */}
        {showBody ? (
          (task.body.trim().length > 0 || task.milestones.length > 0) && (
            <div className="ct-canvas-scroll ct-canvas-fade-in" data-canvas-scroll-body="1" style={{ marginTop: 2, flex: 1, overflowY: "auto", minHeight: 0 }}>
              {task.body.trim().length > 0 && (
                <div data-testid="canvas-task-body" style={atLeast(world, "W3")
                  ? { fontSize: "0.92em", lineHeight: 1.5 }
                  : { fontSize: "0.9em", lineHeight: 1.45, display: "-webkit-box", WebkitLineClamp: 6, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
                  <RichMarkdown text={task.body} />
                </div>
              )}
              {task.milestones.length > 0 && (
                <div style={{ marginTop: task.body.trim() ? 12 : 0 }}>
                  <TaskSteps milestones={task.milestones} colors={stepColors} world={world} />
                </div>
              )}
            </div>
          )
        ) : (
          task.milestones.length > 0 && (
            <div style={{ marginTop: "auto" }}>
              <TaskSteps milestones={task.milestones} colors={stepColors} world={world} />
            </div>
          )
        )}
      </div>
    </div>
  );
}

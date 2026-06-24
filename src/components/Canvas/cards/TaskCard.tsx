import type { CSSProperties } from "react";
import { ExternalLink, Layers, Link2 } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import { atLeast, type World } from "../worlds";
import type { Task, TaskStatus } from "../types";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";
import { StrideSnake } from "./StrideSnake";

// §32 worlds task cell — fixed footprint (§32.4), content adapts to the node's appW
// band (W0 mass → W3 deep). No manual mode, no click-to-reveal: zoom IS navigation.
// §32.3: status/hierarchy is SPATIAL (shelf position, set by the packer), never
// opacity — primary content text is ALWAYS opacity 1.0; done gets a RECESSED
// surface, not a faded one. Opacity is reserved for transient focus only (§32.1).
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

type Shelf = "active" | "normal" | "done";
function shelfOf(status: TaskStatus): Shelf {
  if (status === "in_progress") return "active";
  if (status === "done" || status === "cancelled") return "done";
  return "normal";
}

// Surface chrome by shelf (§32.3): active rises with an accent ring (pulses far);
// done recesses (inset surface + softer header); blocked warns. NO content dim.
function statusChrome(status: TaskStatus, world: World): { ring?: string; pulse: boolean; warnBorder: boolean; recessed: boolean } {
  if (status === "in_progress") {
    return { ring: "0 0 0 1.5px color-mix(in srgb, var(--ct-border-focus) 80%, transparent), 0 6px 22px -8px color-mix(in srgb, var(--ct-border-focus) 55%, transparent)", pulse: !atLeast(world, "W1"), warnBorder: false, recessed: false };
  }
  if (status === "done" || status === "cancelled") return { pulse: false, warnBorder: false, recessed: true };
  if (status === "blocked") return { pulse: false, warnBorder: true, recessed: false };
  return { pulse: false, warnBorder: false, recessed: false };
}

function StatusChip({ color, label, solid }: { color: string; label: string; solid?: boolean }) {
  return (
    <span
      data-testid="canvas-task-status"
      style={{
        fontSize: "0.74em",
        padding: "0 0.5em",
        lineHeight: 1.7,
        borderRadius: 999,
        background: solid ? color : `color-mix(in srgb, ${color} 14%, transparent)`,
        border: `1px solid color-mix(in srgb, ${color} 48%, transparent)`,
        color: solid ? "var(--ct-bg)" : color,
        textTransform: "capitalize",
        fontWeight: 700,
        whiteSpace: "nowrap",
        flexShrink: 0,
      }}
    >
      {label}
    </span>
  );
}

function Counter({ testid, children }: { testid: string; children: React.ReactNode }) {
  return (
    <span data-testid={testid} style={{ display: "inline-flex", alignItems: "center", gap: 3, fontSize: "0.7em", fontWeight: 600, color: "var(--ct-fg)", opacity: 0.78 }}>
      {children}
    </span>
  );
}

function MiniBar({ frac }: { frac: number }) {
  return (
    <span data-testid="canvas-task-progress" style={{ flex: 1, minWidth: 28, height: 5, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
      <span style={{ display: "block", width: `${Math.round(frac * 100)}%`, height: "100%", background: "rgba(82,180,145,0.9)" }} />
    </span>
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

function iconBtn(): CSSProperties {
  return { display: "flex", alignItems: "center", justifyContent: "center", width: 22, height: 22, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
  childCount?: number;
  childDone?: number;
  stepColors?: Map<string, string>;
  world: World;
  onOpenPane?: (paneId: string) => void;
  onHover?: (taskId: string | null) => void;
  onCopyLink?: (taskId: string) => void;
  onCopyStrideLink?: (milestoneId: string) => void;
  onFocusStride?: (milestoneId: string) => void;
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
  onCopyLink,
  onCopyStrideLink,
  onFocusStride,
}: TaskCardProps) {
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);
  const chrome = statusChrome(task.status, world);
  const shelf = shelfOf(task.status);
  const showBody = atLeast(world, "W2");
  const showStructure = atLeast(world, "W1");
  const innerW = (typeof style.width === "number" ? style.width : 260) - 26;

  const strideCount = task.milestones.length;
  const bodyChars = task.body.trim().length;
  const digest = digestMarkdown(task.intent?.trim() ? task.intent! : task.body, 130);
  const freshKind = strideCount ? [...task.milestones].sort((a, b) => b.ts - a.ts)[0].kind : null;
  const freshColor = freshKind ? (stepColors?.get(freshKind) ?? "var(--ct-border-focus)") : null;

  const mergedStyle: CSSProperties = {
    ...cardStyle,
    ...style,
    ...(chrome.recessed ? { background: "color-mix(in srgb, var(--ct-fg) 4%, var(--ct-bg))", boxShadow: "inset 0 1px 0 color-mix(in srgb, var(--ct-fg) 6%, transparent)" } : null),
    ...(chrome.ring ? { boxShadow: chrome.ring } : null),
    ...(chrome.warnBorder ? { borderLeft: "3px solid var(--ct-error, #ef8282)" } : null),
  };

  const actions = showStructure && (
    <div style={{ display: "flex", alignItems: "center", gap: 5, flexShrink: 0 }}>
      {task.assigned_pane_id && (
        <button
          type="button"
          data-testid="canvas-task-pane-link"
          data-pane-id={task.assigned_pane_id}
          title={`Open pane ${task.assigned_pane_id}`}
          onClick={(e) => { e.stopPropagation(); onOpenPane?.(task.assigned_pane_id!); }}
          onMouseDown={(e) => e.stopPropagation()}
          style={iconBtn()}
        >
          <ExternalLink size={11} />
        </button>
      )}
      {showBody && onCopyLink && (
        <button type="button" data-testid="canvas-task-copy-link" title="Copy task link" onClick={(e) => { e.stopPropagation(); onCopyLink(task.id); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
          <Link2 size={11} />
        </button>
      )}
    </div>
  );

  return (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      data-world={world}
      data-shelf={shelf}
      className={`ct-canvas-node${chrome.pulse ? " ct-canvas-live" : ""}`}
      onMouseEnter={() => onHover?.(task.id)}
      onMouseLeave={() => onHover?.(null)}
      style={mergedStyle}
    >
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 7, padding: "10px 12px", minWidth: 0, overflow: "hidden" }}>
        {/* Header — present at every band (the navigation layer). Solid status chip. */}
        <div style={{ display: "flex", alignItems: "flex-start", gap: 8, minWidth: 0 }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: showStructure ? "wrap" : "nowrap", minWidth: 0 }}>
              <span data-testid="canvas-task-name" data-canvas-selectable className="ct-canvas-text" style={{ flex: "1 1 auto", minWidth: 0, maxWidth: "100%", fontWeight: shelf === "done" ? 600 : 700, fontSize: shelf === "done" ? "0.98em" : "1.03em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {task.handle?.trim() || task.name}
              </span>
              <StatusChip color={badgeColor} label={displayStatus} solid />
            </div>
            {showStructure && (task.type || task.model) && (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 6 }}>
                {task.type && <Chip testid="canvas-task-type">{task.type}</Chip>}
                {task.model && <Chip testid="canvas-task-model" mono>{task.model}</Chip>}
              </div>
            )}
          </div>
          {actions}
        </div>

        {/* Digest — present from W0 so a far card is never hollow (§32.2). 2 lines. */}
        {!showBody && digest && (
          <div data-testid="canvas-task-digest" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.84em", lineHeight: 1.4, display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
            {digest}
          </div>
        )}

        {/* W0 counter rail + progress mini-bar + latest-stride bead (§32.2). */}
        {!showStructure && (bodyChars > 0 || strideCount > 0 || childCount > 0) && (
          <div data-testid="canvas-task-counters" style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            {bodyChars > 0 && <Counter testid="canvas-task-body-count">¶ {Math.max(1, Math.round(bodyChars / 80))}</Counter>}
            {strideCount > 0 && (
              <Counter testid="canvas-task-stride-count">
                {freshColor && <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: freshColor }} />}∿ {strideCount}
              </Counter>
            )}
            {childCount > 0 && <Counter testid="canvas-task-subtask-count"><Layers size={10} />⊞ {childCount}</Counter>}
            {childCount > 0 && <MiniBar frac={childDone / childCount} />}
          </div>
        )}

        {/* W2+ intent + model rationale — opacity 1.0 (§32.3), revealed by ZOOM. */}
        {showBody && task.intent && (
          <div data-testid="canvas-task-intent" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.9em", fontStyle: "italic", color: "var(--ct-fg)", opacity: 0.92 }}>
            {task.intent}
          </div>
        )}
        {showBody && task.model_rationale && (
          <div data-testid="canvas-task-why" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.88em", borderLeft: "2px solid var(--ct-border-subtle)", paddingLeft: 8 }}>
            <span style={{ fontWeight: 600, color: "var(--ct-border-focus)" }}>Why this model? </span>
            {task.model_rationale}
          </div>
        )}
        {showBody && childCount > 0 && (
          <div data-testid="canvas-task-subtask-count" style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.74em", fontWeight: 600 }}>
            <span style={{ display: "inline-flex", alignItems: "center", gap: 3 }}><Layers size={11} />⊞ {childCount} subtask{childCount === 1 ? "" : "s"}</span>
            <MiniBar frac={childDone / childCount} />
            <span style={{ fontFamily: "var(--ct-font-mono)", opacity: 0.7 }}>{childDone}/{childCount}</span>
          </div>
        )}

        {/* Body + stride snake. W0/W1 = snake preview pinned to the bottom; W2+ =
            body (clamped at W2, full+scroll at W3) then the blooming snake. */}
        {showBody ? (
          (bodyChars > 0 || strideCount > 0) && (
            <div className="ct-canvas-scroll ct-canvas-fade-in" data-canvas-scroll-body="1" style={{ marginTop: 2, flex: 1, overflowY: "auto", minHeight: 0 }}>
              {bodyChars > 0 && (
                <div data-testid="canvas-task-body" data-canvas-selectable className="ct-canvas-text" style={atLeast(world, "W3")
                  ? { fontSize: "0.92em", lineHeight: 1.5 }
                  : { fontSize: "0.9em", lineHeight: 1.45, display: "-webkit-box", WebkitLineClamp: 6, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
                  <RichMarkdown text={task.body} />
                </div>
              )}
              {strideCount > 0 && (
                <div style={{ marginTop: bodyChars > 0 ? 12 : 0 }}>
                  <StrideSnake milestones={task.milestones} colors={stepColors} world={world} width={innerW} onCopyLink={onCopyStrideLink} onFocusStride={onFocusStride} />
                </div>
              )}
            </div>
          )
        ) : (
          strideCount > 0 && (
            <div style={{ marginTop: "auto" }}>
              <StrideSnake milestones={task.milestones} colors={stepColors} world={world} width={innerW} />
            </div>
          )
        )}
      </div>
    </div>
  );
}

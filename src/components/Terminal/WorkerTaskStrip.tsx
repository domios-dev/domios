import { GitBranch, Hash, Map } from "lucide-react";
import { AgentCircle } from "../Canvas/presence/AgentCircle";
import type { PresenceAgent } from "../Canvas/presence/identity";
import { TASK_STATUS_COLORS } from "../Canvas/settings";
import { TaskStatus, type Task } from "../Canvas/types";

// EMERGENCE (north-star #2), OUTSIDE the canvas: a worker pane wears the ONE task
// it is executing right above its composer — so the swarm is visible without the
// board open, and one click flies you to that task. Honest: no task → no strip.
export interface WorkerTaskStripProps {
  task: Task | null;
  branch?: string | null;
  /** This pane's executor — same color/initials it carries on the board. */
  agent: PresenceAgent | null;
  onOpen?: () => void;
}

function statusText(task: Task): string {
  return task.status_label?.trim() || task.status.replace(/_/g, " ");
}

export function WorkerTaskStrip({ task, branch, agent, onOpen }: WorkerTaskStripProps) {
  if (!task) return null;
  const live = task.status === TaskStatus.InProgress;
  const statusColor = TASK_STATUS_COLORS[task.status];
  const handle = task.handle?.trim() || null;
  // Tint the strip's leading rail in the agent's identity hue — the one signature
  // tying "who is working" to "what's in flight"; everything else stays quiet.
  const accent = agent?.color ?? statusColor;
  return (
    <button
      type="button"
      data-testid="worker-task-strip"
      data-task-id={task.id}
      data-task-status={task.status}
      title={`${statusText(task)} · ${task.name}${branch ? ` · ${branch}` : ""}\nOpen canvas and fly to this task`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onOpen}
      className="mb-2 flex w-full min-w-0 items-center gap-2 overflow-hidden rounded-[0.9rem] border py-1 pl-2 pr-2.5 text-left transition hover:brightness-110"
      style={{
        background: "var(--ct-bg-elev, var(--ct-surface, #161b22))",
        borderColor: `color-mix(in srgb, ${accent} 38%, var(--ct-border, #30363d))`,
        borderLeft: `3px solid ${accent}`,
        cursor: onOpen ? "pointer" : "default",
      }}
    >
      {agent && <AgentCircle agent={agent} size={16} live={live} ring={false} />}
      {handle && (
        <span
          data-testid="worker-task-strip-handle"
          className="inline-flex shrink-0 items-center gap-0.5 font-mono"
          style={{
            maxWidth: 90,
            fontSize: "0.66rem",
            fontWeight: 600,
            color: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 60%, transparent)",
          }}
        >
          <Hash size={9} strokeWidth={2.5} aria-hidden className="shrink-0 opacity-70" />
          <span className="truncate">{handle}</span>
        </span>
      )}
      <span
        data-testid="worker-task-strip-name"
        data-task-name-full={task.name}
        className="min-w-0 flex-1 truncate"
        style={{ fontSize: "0.78rem", fontWeight: 600, color: "var(--ct-fg, #c9d1d9)" }}
      >
        {task.name}
      </span>
      <span
        data-testid="worker-task-strip-status"
        className="shrink-0 capitalize"
        style={{
          fontSize: "0.62rem",
          fontWeight: 700,
          lineHeight: 1.7,
          padding: "0 0.5em",
          borderRadius: 999,
          whiteSpace: "nowrap",
          background: statusColor,
          color: "var(--ct-bg, #0d1117)",
        }}
      >
        {statusText(task)}
      </span>
      {branch && (
        <span
          data-testid="worker-task-strip-branch"
          className="inline-flex min-w-0 max-w-[40%] items-center gap-1 font-mono"
          style={{
            flexShrink: 1,
            padding: "0.05em 0.42em",
            borderRadius: "var(--ct-radius-sm, 6px)",
            border: "1px solid color-mix(in srgb, rgba(96,165,250,0.95) 40%, transparent)",
            background: "color-mix(in srgb, rgba(96,165,250,0.95) 12%, transparent)",
            fontSize: "0.64rem",
            fontWeight: 600,
            color: "rgba(96,165,250,0.95)",
          }}
        >
          <GitBranch size={10} strokeWidth={2.4} aria-hidden className="shrink-0" />
          <span className="truncate">{branch}</span>
        </span>
      )}
      <Map size={12} aria-hidden className="shrink-0 opacity-50" style={{ color: "var(--ct-muted-fg, #8b949e)" }} />
    </button>
  );
}

import type { CSSProperties, ReactNode } from "react";
import { ArrowRight, CheckCircle2, Clock3, FolderGit2, GitBranch, Hash, Trash2, TriangleAlert, Unplug } from "lucide-react";
import { atLeast, World } from "../worlds";
import { TaskPriority, TaskStatus, type DriftItem, type DriftKind, type TaskLaneMetadata } from "../types";
import { Eyebrow } from "./provenanceShell";
import type { TaskPresence } from "../taskPresence";
import { AgentStack } from "../presence/AgentCircle";

// Shared presentational parts for the task card (extracted from TaskCard.tsx, Phase C
// iter3) — pure helpers/consts used by both the schematic map face and the detail tree.
const SLOT_TRANSITION = "left 0.3s var(--ct-ease-decel), top 0.3s var(--ct-ease-decel)";

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
  transition: SLOT_TRANSITION,
};

function statusLabel(status: TaskStatus): string {
  return status.replace(/_/g, " ");
}

const Shelf = { Active: "active", Normal: "normal", Done: "done" } as const;
type Shelf = (typeof Shelf)[keyof typeof Shelf];
function shelfOf(status: TaskStatus): Shelf {
  if (status === TaskStatus.InProgress) return Shelf.Active;
  if (status === TaskStatus.Done || status === TaskStatus.Cancelled) return Shelf.Done;
  return Shelf.Normal;
}

// Surface chrome by shelf (§32.3): active rises with an accent ring (pulses far);
// done recesses (inset surface + softer header); blocked warns. NO content dim.
function statusChrome(status: TaskStatus, world: World): { ring?: string; pulse: boolean; warnBorder: boolean; recessed: boolean } {
  if (status === TaskStatus.InProgress) {
    return { ring: "0 0 0 1.5px color-mix(in srgb, var(--ct-border-focus) 80%, transparent), 0 6px 22px -8px color-mix(in srgb, var(--ct-border-focus) 55%, transparent)", pulse: !atLeast(world, World.W1), warnBorder: false, recessed: false };
  }
  if (status === TaskStatus.InReview) {
    return { ring: "0 0 0 1.5px color-mix(in srgb, rgba(245, 158, 11, 0.92) 78%, transparent)", pulse: false, warnBorder: false, recessed: false };
  }
  if (status === TaskStatus.Done || status === TaskStatus.Cancelled) return { pulse: false, warnBorder: false, recessed: true };
  if (status === TaskStatus.Blocked) return { pulse: false, warnBorder: true, recessed: false };
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

function Counter({ testid, children }: { testid: string; children: ReactNode }) {
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

function Chip({ testid, mono, accent, icon, max, children }: { testid: string; mono?: boolean; accent?: string; icon?: ReactNode; max?: number; children: ReactNode }) {
  return (
    <span
      data-testid={testid}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: icon ? 4 : 0,
        maxWidth: max,
        fontSize: "0.78em",
        fontFamily: mono ? "var(--ct-font-mono)" : undefined,
        padding: "0.08em 0.45em",
        borderRadius: "var(--ct-radius-sm)",
        border: accent ? `1px solid color-mix(in srgb, ${accent} 48%, transparent)` : "1px solid var(--ct-border-subtle)",
        background: accent ? `color-mix(in srgb, ${accent} 14%, transparent)` : mono ? "var(--ct-bg-selected)" : "var(--ct-bg-active)",
        color: accent ?? "var(--ct-fg)",
        textTransform: mono ? "none" : "capitalize",
        whiteSpace: "nowrap",
      }}
    >
      {icon}
      <span style={max ? { minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } : undefined}>{children}</span>
    </span>
  );
}

// Priority (§seed gap): p0 highest. Reuses the product's semantic color language —
// red = urgent (drift/error family), amber = attention (in_review) — so no new
// vocabulary. p0 alarms (solid); p1 attention (amber tint); p2/p3 stay quiet.
const PRIORITY_META: Record<TaskPriority, { label: string; color: string; solid: boolean }> = {
  [TaskPriority.P0]: { label: "P0", color: "rgba(248,113,113,0.98)", solid: true },
  [TaskPriority.P1]: { label: "P1", color: "rgba(245,158,11,0.97)", solid: false },
  [TaskPriority.P2]: { label: "P2", color: "var(--ct-fg)", solid: false },
  [TaskPriority.P3]: { label: "P3", color: "var(--ct-fg)", solid: false },
};

function PriorityTag({ priority, dim }: { priority: TaskPriority; dim: boolean }) {
  const m = PRIORITY_META[priority];
  const quiet = priority === TaskPriority.P2 || priority === TaskPriority.P3;
  return (
    <span
      data-testid="canvas-task-priority"
      data-priority={priority}
      title={`Priority ${m.label}${priority === TaskPriority.P0 ? " · highest" : priority === TaskPriority.P3 ? " · lowest" : ""}`}
      style={{
        flexShrink: 0,
        fontSize: "0.66em",
        fontWeight: 800,
        letterSpacing: "0.05em",
        fontFamily: "var(--ct-font-mono)",
        lineHeight: 1.55,
        padding: "0 0.4em",
        borderRadius: "var(--ct-radius-sm)",
        background: m.solid ? m.color : `color-mix(in srgb, ${m.color} ${quiet ? 7 : 16}%, transparent)`,
        border: `1px solid color-mix(in srgb, ${m.color} ${quiet ? 28 : 55}%, transparent)`,
        color: m.solid ? "var(--ct-bg)" : quiet ? "color-mix(in srgb, var(--ct-fg) 68%, transparent)" : m.color,
        opacity: dim ? 0.45 : 1,
      }}
    >
      {m.label}
    </span>
  );
}

// EMERGENCE (north-star #2): the on-card "who is here" read. Color-keyed agent
// circles (multiplayer) + the lead handle + the BRANCH, glanceable from W1 — so the
// swarm is visible without diving to the W3 lane block. Activity (latest step kind)
// rides along once there's room (W2+). `live` pulses the lead when executing.
function presenceTooltip(presence: TaskPresence): string {
  return [
    `Working: ${presence.agents.map((a) => a.label + (a.role ? ` (${a.role})` : "")).join(", ") || presence.label}`,
    `Pane: ${presence.paneId}`,
    presence.branch ? `Branch: ${presence.branch}` : null,
    presence.focus ? `Focus: ${presence.focus}` : null,
    presence.latest ? `Latest: ${presence.latest.authorLabel} · ${presence.latest.kindLabel}` : null,
  ].filter(Boolean).join("\n");
}

function PresenceCluster({ presence, live, showActivity, compact = false }: { presence: TaskPresence; live: boolean; showActivity: boolean; compact?: boolean }) {
  const lead = presence.agents[0];
  const leadLabel = lead?.label ?? presence.label;
  const activity = showActivity && presence.latest ? presence.latest.kindLabel : null;
  const title = presenceTooltip(presence);
  return (
    <span
      data-testid="canvas-task-presence"
      data-pane-id={presence.paneId}
      data-agent-count={presence.agents.length}
      title={title}
      style={{
        display: "flex",
        alignItems: "center",
        flexWrap: compact ? "nowrap" : "wrap",
        gap: 6,
        rowGap: 4,
        minWidth: 0,
        maxWidth: "100%",
        fontSize: "0.72em",
        fontWeight: 700,
        color: "var(--ct-fg)",
      }}
    >
      {presence.agents.length > 0 && <AgentStack agents={presence.agents} size={compact ? 15 : 18} live={live} />}
      <span data-testid="canvas-task-presence-detail" style={{ minWidth: 0, maxWidth: "100%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {leadLabel}
      </span>
      {!compact && presence.branch && (
        <span
          data-testid="canvas-task-presence-branch"
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 3,
            minWidth: 0,
            maxWidth: "100%",
            padding: "0.05em 0.42em",
            borderRadius: "var(--ct-radius-sm)",
            border: "1px solid color-mix(in srgb, rgba(96,165,250,0.95) 40%, transparent)",
            background: "color-mix(in srgb, rgba(96,165,250,0.95) 12%, transparent)",
            fontFamily: "var(--ct-font-mono)",
            fontWeight: 600,
            color: "rgba(96,165,250,0.95)",
          }}
        >
          <GitBranch size="0.78em" strokeWidth={2.4} aria-hidden style={{ flexShrink: 0 }} />
          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{presence.branch}</span>
        </span>
      )}
      {activity && (
        <span data-testid="canvas-task-presence-activity" style={{ minWidth: 0, fontWeight: 600, color: "color-mix(in srgb, var(--ct-fg) 55%, transparent)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {activity}
        </span>
      )}
    </span>
  );
}

// The lane `handle` is the orchestrator's canonical short-id for the task (the slug
// CLI commands address). It rides as a dashed identity token — never a value chip —
// so it reads as a name tag at glance and the full slug stays one hover away.
function HandleChip({ handle, dim }: { handle: string; dim?: boolean }) {
  return (
    <span
      data-testid="canvas-task-handle"
      title={`Handle: ${handle}`}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 2,
        maxWidth: 132,
        fontSize: "0.7em",
        fontFamily: "var(--ct-font-mono)",
        fontWeight: 600,
        padding: "0.05em 0.42em 0.05em 0.28em",
        borderRadius: "var(--ct-radius-sm)",
        border: "1px dashed var(--ct-border-subtle)",
        color: "color-mix(in srgb, var(--ct-fg) 64%, transparent)",
        opacity: dim ? 0.5 : 1,
        flexShrink: 0,
      }}
    >
      <Hash size="0.7em" strokeWidth={2.5} aria-hidden style={{ opacity: 0.7, flexShrink: 0 }} />
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{handle}</span>
    </span>
  );
}

// cleanup_status reuses the product's semantic palette: amber = pending/attention,
// green = settled, red = failed; an unrecognised/not-applicable status stays neutral.
function cleanupTone(status: string): string {
  const s = status.toLowerCase();
  if (s.includes("fail") || s.includes("error")) return "rgba(248,113,113,0.98)";
  if (s.includes("pending") || s.includes("await")) return "rgba(245,158,11,0.97)";
  if (s.includes("done") || s.includes("complete") || s.includes("clean")) return "rgba(82,180,145,0.95)";
  return "color-mix(in srgb, var(--ct-fg) 60%, transparent)";
}

function LaneRow({ icon, testid, children }: { icon: ReactNode; testid?: string; children: ReactNode }) {
  return (
    <div data-testid={testid} style={{ display: "flex", alignItems: "flex-start", gap: 6, minWidth: 0, fontSize: "0.86em", lineHeight: 1.45 }}>
      <span aria-hidden style={{ display: "inline-flex", marginTop: 2, color: "color-mix(in srgb, var(--ct-fg) 50%, transparent)", flexShrink: 0 }}>{icon}</span>
      <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>{children}</span>
    </div>
  );
}

// §deep-dive (W3): the full lane provenance — where the work physically lives
// (workspace + worktree), how the branch flows (base → destination), and what
// cleanup it still owes. Reveal-by-zoom; rendered only when a field is present.
function LaneDetail({ lane }: { lane: TaskLaneMetadata }) {
  const workspace = lane.workspace_name?.trim() || null;
  const worktree = lane.worktree_path?.trim() || null;
  const branch = lane.branch?.trim() || null;
  const base = lane.base?.trim() || null;
  const destination = lane.destination?.trim() || null;
  const cleanup = lane.cleanup_status?.trim() || null;
  const hints = Array.isArray(lane.cleanup_hints) ? lane.cleanup_hints.filter((h) => h && h.trim()) : [];
  const muted = "color-mix(in srgb, var(--ct-fg) 55%, transparent)";
  const mono = { fontFamily: "var(--ct-font-mono)" } as const;
  return (
    <div data-testid="canvas-task-lane" style={{ display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--ct-border-subtle)", paddingTop: 8, marginTop: 2 }}>
      <Eyebrow icon={<FolderGit2 size="0.92em" />} label="Lane" accent="rgba(96,165,250,0.95)" testid="canvas-task-lane-eyebrow" />
      {(workspace || worktree) && (
        <LaneRow icon={<FolderGit2 size="0.92em" />} testid="canvas-task-lane-workspace">
          {workspace && <span style={{ fontWeight: 600 }}>{workspace}</span>}
          {worktree && <span style={{ ...mono, color: muted }}>{workspace ? " · " : ""}{worktree}</span>}
        </LaneRow>
      )}
      {(branch || base || destination) && (
        <LaneRow icon={<GitBranch size="0.92em" />} testid="canvas-task-lane-branch">
          {branch && <span style={mono}>{branch}</span>}
          {(base || destination) && (
            <span style={{ ...mono, color: muted, display: "inline-flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
              {branch ? <span style={{ opacity: 0.6 }}>·</span> : null}
              {base && <span>{base}</span>}
              {base && destination && <ArrowRight size="0.85em" aria-hidden style={{ flexShrink: 0 }} />}
              {destination && <span>{destination}</span>}
            </span>
          )}
        </LaneRow>
      )}
      {cleanup && (
        <LaneRow icon={<Trash2 size="0.92em" />} testid="canvas-task-lane-cleanup">
          <span style={{ color: muted }}>cleanup </span>
          <span style={{ color: cleanupTone(cleanup), fontWeight: 600 }}>{cleanup.replace(/_/g, " ")}</span>
          {hints.length > 0 && (
            <ul style={{ margin: "4px 0 0", paddingLeft: 16, color: muted, fontSize: "0.94em" }}>
              {hints.map((h, i) => <li key={i} style={{ overflowWrap: "anywhere" }}>{h}</li>)}
            </ul>
          )}
        </LaneRow>
      )}
    </div>
  );
}

function iconBtn(): CSSProperties {
  return { display: "flex", alignItems: "center", justifyContent: "center", width: 22, height: 22, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

function revealStyle(visible: boolean): CSSProperties {
  return visible ? {} : { visibility: "hidden", opacity: 0, pointerEvents: "none" };
}

const DRIFT_CHROME: Record<DriftKind, { Icon: typeof TriangleAlert; color: string; label: string }> = {
  in_progress_pane_gone: { Icon: Unplug, color: "rgba(248,113,113,0.98)", label: "Worker pane gone" },
  stale_in_review: { Icon: Clock3, color: "rgba(245,158,11,0.98)", label: "Stale review" },
  bound_to_closed_pane: { Icon: TriangleAlert, color: "rgba(251,191,36,0.98)", label: "Closed pane binding" },
  accepted_but_not_done: { Icon: CheckCircle2, color: "rgba(96,165,250,0.98)", label: "Accepted but not done" },
};

function DriftBadge({ drift }: { drift?: DriftItem }) {
  const chrome = drift ? DRIFT_CHROME[drift.kind] : null;
  const Icon = chrome?.Icon ?? TriangleAlert;
  const color = drift?.not_mine ? "var(--ct-fg)" : chrome?.color ?? "var(--ct-border-focus)";
  const title = drift ? `${chrome?.label ?? drift.kind.replace(/_/g, " ")}: ${drift.detail}${drift.suggested_fix ? `\nSuggested: ${drift.suggested_fix}` : ""}` : "";
  return (
    <span
      data-testid={drift ? "canvas-task-drift-badge" : "canvas-task-drift-slot"}
      data-drift-kind={drift?.kind}
      data-not-mine={drift ? String(drift.not_mine) : undefined}
      title={title}
      aria-label={title || undefined}
      style={{
        width: 20,
        height: 20,
        borderRadius: "var(--ct-radius-sm)",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        color,
        background: drift?.not_mine ? "color-mix(in srgb, var(--ct-fg) 7%, transparent)" : `color-mix(in srgb, ${color} 15%, transparent)`,
        border: `1px solid ${drift?.not_mine ? "var(--ct-border-subtle)" : `color-mix(in srgb, ${color} 55%, transparent)`}`,
        opacity: drift ? (drift.not_mine ? 0.55 : 1) : 0,
        visibility: drift ? "visible" : "hidden",
        pointerEvents: drift ? "auto" : "none",
      }}
    >
      <Icon size="0.92em" strokeWidth={2.4} />
    </span>
  );
}

export { cardStyle, statusLabel, Shelf, shelfOf, statusChrome, StatusChip, Counter, MiniBar, Chip, PriorityTag, presenceTooltip, PresenceCluster, HandleChip, LaneDetail, iconBtn, revealStyle, DriftBadge };

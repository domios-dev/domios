import type { CSSProperties, ReactNode } from "react";
import { ArrowRight, CheckCircle2, Clock3, CornerDownRight, ExternalLink, FolderGit2, GitBranch, Hash, Layers, Link2, Trash2, TriangleAlert, Unplug, UserRound, Users } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import { atLeast, DEEP_DIVE_CRISP_SCALE, World } from "../worlds";
import { TaskPriority, TaskStatus, type DriftItem, type DriftKind, type TagFacet, type Task, type TaskLaneMetadata } from "../types";
import { resolveTags } from "../tagModel";
import type { CanvasCardStyleProps } from "./types";
import { Eyebrow } from "./provenanceShell";
import { digestMarkdown } from "./digest";
import { formatTaskFreshness, taskFreshnessAt } from "./taskFreshness";
import { StrideSnake } from "./StrideSnake";
import { formatTime } from "../../../lib/formatters";
import type { TaskPresence } from "../taskPresence";
import { AgentStack } from "../presence/AgentCircle";

// §32 worlds task cell — fixed footprint (§32.4), content adapts to the node's appW
// band (W0 mass → W3 deep). No manual mode, no click-to-reveal: zoom IS navigation.
// §32.3: status/hierarchy is SPATIAL (shelf position, set by the packer), never
// opacity — primary content text is ALWAYS opacity 1.0; done gets a RECESSED
// surface, not a faded one. Opacity is reserved for transient focus only (§32.1).
// §32.10 slot animation: a task re-slotting (status/priority change) eases to its
// new cloud position — fires only on data repack, never on pan/zoom (camera fixed).
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

function Chip({ testid, mono, accent, icon, max, children }: { testid: string; mono?: boolean; accent?: string; icon?: React.ReactNode; max?: number; children: React.ReactNode }) {
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
function PresenceCluster({ presence, live, showActivity }: { presence: TaskPresence; live: boolean; showActivity: boolean }) {
  const lead = presence.agents[0];
  const leadLabel = lead?.label ?? presence.label;
  const activity = showActivity && presence.latest ? presence.latest.kindLabel : null;
  const title = [
    `Working: ${presence.agents.map((a) => a.label + (a.role ? ` (${a.role})` : "")).join(", ") || presence.label}`,
    `Pane: ${presence.paneId}`,
    presence.branch ? `Branch: ${presence.branch}` : null,
    presence.focus ? `Focus: ${presence.focus}` : null,
    presence.latest ? `Latest: ${presence.latest.authorLabel} · ${presence.latest.kindLabel}` : null,
  ].filter(Boolean).join("\n");
  return (
    <span
      data-testid="canvas-task-presence"
      data-pane-id={presence.paneId}
      data-agent-count={presence.agents.length}
      title={title}
      style={{
        display: "flex",
        alignItems: "center",
        flexWrap: "wrap",
        gap: 6,
        rowGap: 4,
        minWidth: 0,
        maxWidth: "100%",
        fontSize: "0.72em",
        fontWeight: 700,
        color: "var(--ct-fg)",
      }}
    >
      {presence.agents.length > 0 && <AgentStack agents={presence.agents} size={18} live={live} />}
      <span data-testid="canvas-task-presence-detail" style={{ minWidth: 0, maxWidth: "100%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {leadLabel}
      </span>
      {presence.branch && (
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
          <GitBranch size={10} strokeWidth={2.4} aria-hidden style={{ flexShrink: 0 }} />
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

function PrimaryFacetChip({ label, color }: { label: string; color: string }) {
  return (
    <span
      data-testid="canvas-task-primary-facet"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        fontSize: "0.8em",
        fontWeight: 700,
        padding: "0.1em 0.5em",
        borderRadius: 999,
        border: `1px solid color-mix(in srgb, ${color} 55%, transparent)`,
        background: `color-mix(in srgb, ${color} 16%, transparent)`,
        color: "var(--ct-fg)",
        whiteSpace: "nowrap",
      }}
    >
      <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />
      {label}
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
      <Hash size={9} strokeWidth={2.5} aria-hidden style={{ opacity: 0.7, flexShrink: 0 }} />
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
      <Eyebrow icon={<FolderGit2 size={12} />} label="Lane" accent="rgba(96,165,250,0.95)" testid="canvas-task-lane-eyebrow" />
      {(workspace || worktree) && (
        <LaneRow icon={<FolderGit2 size={12} />} testid="canvas-task-lane-workspace">
          {workspace && <span style={{ fontWeight: 600 }}>{workspace}</span>}
          {worktree && <span style={{ ...mono, color: muted }}>{workspace ? " · " : ""}{worktree}</span>}
        </LaneRow>
      )}
      {(branch || base || destination) && (
        <LaneRow icon={<GitBranch size={12} />} testid="canvas-task-lane-branch">
          {branch && <span style={mono}>{branch}</span>}
          {(base || destination) && (
            <span style={{ ...mono, color: muted, display: "inline-flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
              {branch ? <span style={{ opacity: 0.6 }}>·</span> : null}
              {base && <span>{base}</span>}
              {base && destination && <ArrowRight size={11} aria-hidden style={{ flexShrink: 0 }} />}
              {destination && <span>{destination}</span>}
            </span>
          )}
        </LaneRow>
      )}
      {cleanup && (
        <LaneRow icon={<Trash2 size={12} />} testid="canvas-task-lane-cleanup">
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
      <Icon size={12} strokeWidth={2.4} />
    </span>
  );
}

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
  tagFacets?: TagFacet[];
  childCount?: number;
  childDone?: number;
  parentTaskName?: string | null;
  stepColors?: Map<string, string>;
  drift?: DriftItem;
  presence?: TaskPresence | null;
  world: World;
  /** §34 crisp text: >1 → re-rasterize at on-screen size (zoom + counter-scale). */
  crispScale?: number;
  onOpenPane?: (paneId: string) => void;
  onHover?: (taskId: string | null) => void;
  onCopyLink?: (taskId: string) => void;
  onCopyStrideLink?: (milestoneId: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  tagFilterDimmed?: boolean;
  tagFilterHighlighted?: boolean;
  suppressRichContent?: boolean;
  richOverlay?: boolean;
}

export function TaskCard({
  task,
  tagFacets,
  childCount = 0,
  childDone = 0,
  parentTaskName = null,
  stepColors,
  drift,
  presence,
  style,
  world,
  crispScale = 1,
  onOpenPane,
  onHover,
  onCopyLink,
  onCopyStrideLink,
  onFocusStride,
  tagFilterDimmed = false,
  tagFilterHighlighted = false,
  suppressRichContent = false,
  richOverlay = false,
}: TaskCardProps) {
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);
  const chrome = statusChrome(task.status, world);
  const shelf = shelfOf(task.status);
  const showTitle = atLeast(world, World.W1);
  const showStructure = atLeast(world, World.W2);
  const isDone = shelf === Shelf.Done;
  // Urgent (p0/p1) leads the title from W1 — the overview must shout what's on
  // fire. The quieter p2/p3 ranks ride a meta chip at W2, so they don't stamp every
  // card or steal title space on a narrow tile.
  const isUrgent = task.priority === TaskPriority.P0 || task.priority === TaskPriority.P1;
  const titlePriority = !!task.priority && isUrgent;
  const metaPriority = !!task.priority && !isUrgent && showStructure;
  const role = task.taken_by_role?.trim() || null;
  const lane = task.lane;
  const branch = lane?.branch?.trim() || null;
  const handle = task.handle?.trim() || null;
  const group = task.group?.trim() || null;
  const peer = task.peer?.trim() || null;
  const hasLaneDetail = !!(branch || lane?.workspace_name?.trim() || lane?.worktree_path?.trim() || lane?.base?.trim() || lane?.destination?.trim() || lane?.cleanup_status?.trim() || (lane?.cleanup_hints?.some((h) => h && h.trim())));
  const budgetRich = suppressRichContent && !richOverlay;
  const showBodySlot = atLeast(world, World.W2) && !budgetRich;
  const showFullBody = atLeast(world, World.W3) && !budgetRich;
  const showDeepDive = showFullBody && crispScale >= DEEP_DIVE_CRISP_SCALE;
  const innerW = (typeof style.width === "number" ? style.width : 260) - 26;

  const strideCount = task.milestones.length;
  const bodyChars = task.body.trim().length;
  const acItems = Array.isArray(task.acceptance_criteria) ? task.acceptance_criteria.filter((c) => c && c.trim()) : [];
  const testPlan = typeof task.test_plan === "string" ? task.test_plan.trim() : "";
  const hasFields = acItems.length > 0 || testPlan.length > 0;
  const digest = digestMarkdown(task.intent?.trim() ? task.intent! : task.body, 130);
  const freshKind = strideCount ? [...task.milestones].sort((a, b) => b.ts - a.ts)[0].kind : null;
  const freshColor = freshKind ? (stepColors?.get(freshKind) ?? "var(--ct-border-focus)") : null;
  const titleText = task.name;
  // The worlds promise: dive in → read everything. Title is the card's identity, so
  // it shows from the farthest band (W0) and un-clamps entirely at W3 (the deep-read
  // document scrolls, so a long title is never cut). W1/W2 widen the clamp as the
  // card earns the pixels.
  const nameClamp = showFullBody ? null : showStructure ? 4 : 2;
  const resolved = resolveTags(task.tags, tagFacets);
  const tileTags = !showBodySlot && (resolved.primary || resolved.topics.length > 0);
  const topicTile = resolved.topics.slice(0, 2);
  const freshnessAt = taskFreshnessAt(task);
  const freshnessLabel = freshnessAt != null ? formatTaskFreshness(freshnessAt) : "";
  const showStrides = strideCount > 0 && (showBodySlot || !showStructure);
  const contentRows = showBodySlot ? "auto minmax(0, auto) 0 minmax(28px, 1fr)" : "auto 26px 42px minmax(0, 1fr)";
  const contentGap = showBodySlot ? 4 : 7;
  const contentPadding = showBodySlot ? "8px 10px" : "10px 12px";
  const bodyRows = showFullBody
    ? (showDeepDive ? "auto minmax(0, 1fr)" : "minmax(36px, 0.8fr) minmax(28px, 1fr)")
    : (showBodySlot
      ? (showStrides ? "minmax(36px, 1fr) minmax(28px, 1fr)" : "minmax(36px, 1fr) 0")
      : "minmax(36px, 0.8fr) minmax(28px, 1fr)");
  const bodyGap = showFullBody || !showBodySlot ? 10 : (showStrides ? 10 : 0);

  // chrome overrides win over the box geometry (§32.3 surface chrome by shelf).
  const chromeOverrides: CSSProperties = {
    ...(chrome.recessed ? { background: "color-mix(in srgb, var(--ct-fg) 4%, var(--ct-bg))", boxShadow: "inset 0 1px 0 color-mix(in srgb, var(--ct-fg) 6%, transparent)" } : null),
    ...(chrome.ring ? { boxShadow: chrome.ring } : null),
    ...(chrome.warnBorder ? { borderLeft: "3px solid var(--ct-error, #ef8282)" } : null),
    ...(!chrome.warnBorder && resolved.primary?.color ? { borderLeftWidth: 3, borderLeftStyle: "solid", borderLeftColor: resolved.primary.color } : null),
    // Ambient urgency: p0/p1 with no facet/blocked rail get a priority-tinted edge,
    // so a fire is catchable in the W0/W1 cloud before the tag itself reveals.
    ...(!chrome.warnBorder && !resolved.primary?.color && !isDone && isUrgent && task.priority
      ? { borderLeftWidth: 3, borderLeftStyle: "solid", borderLeftColor: PRIORITY_META[task.priority].color }
      : null),
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
      {showBodySlot && task.section_ids.length > 0 && (
        <button
          type="button"
          data-testid="canvas-task-section-link"
          data-fly-to-kind="task-section"
          title="Fly to the plan section this task is bound to"
          onMouseDown={(e) => e.stopPropagation()}
          style={iconBtn()}
        >
          <Layers size={11} />
        </button>
      )}
      {showBodySlot && onCopyLink && (
        <button type="button" data-testid="canvas-task-copy-link" title="Copy task link" onClick={(e) => { e.stopPropagation(); onCopyLink(task.id); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
          <Link2 size={11} />
        </button>
      )}
    </div>
  );

  // §header: title is the card's identity, so it earns a full-width line. At W2+ the
  // status/drift/actions ride an eyebrow row ABOVE it (label → headline) so a long
  // title reads to the card edge instead of fighting a chip for ~50px. W0/W1 stay
  // compact inline — the glance band where the chip is the signal and the title is
  // short-clamped anyway.
  const nameSpan = (
    <span data-testid="canvas-task-name" data-visible="true" data-task-name-full={task.name} data-canvas-selectable className="ct-canvas-text" style={{ minWidth: 0, maxWidth: "100%", fontWeight: shelf === Shelf.Done ? 600 : 700, fontSize: shelf === Shelf.Done ? "0.98em" : "1.03em", lineHeight: 1.25, overflowWrap: "break-word", ...(nameClamp != null ? { display: "-webkit-box", WebkitLineClamp: nameClamp, WebkitBoxOrient: "vertical", overflow: "hidden" } : { display: "block" }) }}>
      {titlePriority && task.priority && (
        <span style={{ marginRight: 5, verticalAlign: "1px" }}><PriorityTag priority={task.priority} dim={isDone} /></span>
      )}
      {titleText}
    </span>
  );
  const statusCluster = (
    <div style={{ display: "inline-flex", alignItems: "center", gap: 4, flexShrink: 0 }}>
      <StatusChip color={badgeColor} label={displayStatus} solid />
      <DriftBadge drift={drift} />
    </div>
  );
  // EMERGENCE: presence lives in the HEADER (never collapses, present every band)
  // so "who is here + branch" is glanceable from W1 — the W2+ summary slot collapses
  // to 0, so the counters row is the wrong home. Hidden at W0 (mass glance).
  const presenceEl = presence && showTitle
    ? <PresenceCluster presence={presence} live={task.status === TaskStatus.InProgress} showActivity={showStructure} />
    : null;

  const card = (boxStyle: CSSProperties) => (
    <div
      data-testid="canvas-task-card"
      data-task-id={task.id}
      data-world={world}
      data-shelf={shelf}
      data-tag-filter={tagFilterDimmed ? "dimmed" : tagFilterHighlighted ? "highlight" : undefined}
      className={`ct-canvas-node${chrome.pulse ? " ct-canvas-live" : ""}${tagFilterDimmed ? " ct-canvas-tag-dimmed" : ""}`}
      onMouseEnter={() => onHover?.(task.id)}
      onMouseLeave={() => onHover?.(null)}
      style={{ ...cardStyle, ...boxStyle, ...chromeOverrides, ...(tagFilterDimmed ? { opacity: 0.22, pointerEvents: "none" as const } : {}) }}
    >
      <div
        className={showFullBody ? "ct-canvas-scroll" : undefined}
        data-canvas-scroll-body={showFullBody ? "1" : undefined}
        style={showFullBody
          ? { flex: 1, display: "flex", flexDirection: "column", gap: contentGap, padding: contentPadding, minWidth: 0, overflowY: "auto" }
          : { flex: 1, display: "grid", gridTemplateRows: contentRows, gridTemplateColumns: "minmax(0, 1fr)", gap: contentGap, padding: contentPadding, minWidth: 0, overflow: "hidden" }}
      >
        {/* Header — present at every band (the navigation layer). See §header above. */}
        {showStructure ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 5, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
              {statusCluster}
              <span style={{ flex: 1, minWidth: 0 }} />
              {actions}
            </div>
            {nameSpan}
            {presenceEl}
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 5, minWidth: 0 }}>
            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", alignItems: "start", gap: 6, minWidth: 0 }}>
              {nameSpan}
              {statusCluster}
            </div>
            {presenceEl}
          </div>
        )}

        <div data-testid="canvas-task-meta-slot" style={{ display: "flex", flexWrap: "wrap", alignContent: "flex-start", gap: 6, minHeight: 0, maxHeight: showFullBody ? undefined : 44, overflow: "hidden", ...revealStyle(showStructure) }}>
          {metaPriority && task.priority && <span style={{ display: "inline-flex", alignItems: "center" }}><PriorityTag priority={task.priority} dim={isDone} /></span>}
          {handle && <HandleChip handle={handle} dim={isDone} />}
          {task.type && <Chip testid="canvas-task-type">{task.type}</Chip>}
          {role && <Chip testid="canvas-task-role" accent="rgba(96,165,250,0.95)" icon={<UserRound size={11} aria-hidden />} max={150}>{role}</Chip>}
          {branch && !presence?.branch && <Chip testid="canvas-task-branch" mono icon={<GitBranch size={11} aria-hidden />} max={170}>{branch}</Chip>}
          {peer && !presence?.agents.some((a) => a.isPeer) && <Chip testid="canvas-task-peer" icon={<Users size={11} aria-hidden />} max={140}>{peer}</Chip>}
          {group && <Chip testid="canvas-task-group" max={140}>{group}</Chip>}
          {task.model && <Chip testid="canvas-task-model" mono>{task.model}</Chip>}
          {resolved.facets.map((facet) => (
            <Chip key={facet.tag} testid="canvas-task-tag-facet" accent={facet.color ?? undefined}>{facet.label}</Chip>
          ))}
          {resolved.topics.map((topic) => <Chip key={topic} testid="canvas-task-tag">{topic}</Chip>)}
        </div>

        <div data-testid="canvas-task-summary-slot" style={{ minHeight: 0, overflow: "hidden" }}>
          {tileTags ? (
            <div data-testid="canvas-task-tile-tags" style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center", ...revealStyle(true) }}>
              {resolved.primary?.color && <PrimaryFacetChip label={resolved.primary.label} color={resolved.primary.color} />}
              {resolved.primary && !resolved.primary.color && <Chip testid="canvas-task-tag-facet">{resolved.primary.label}</Chip>}
              {topicTile.map((topic) => <Chip key={topic} testid="canvas-task-tag">{topic}</Chip>)}
            </div>
          ) : !showBodySlot && digest ? (
            <div data-testid="canvas-task-digest" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.84em", lineHeight: 1.4, display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden", ...revealStyle(showTitle) }}>
            {digest}
            </div>
          ) : null}

          <div data-testid="canvas-task-counters" style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", ...revealStyle(bodyChars > 0 || strideCount > 0 || childCount > 0 || freshnessLabel.length > 0 || !!parentTaskName || (!!handle && showTitle)) }}>
            {handle && showTitle && <HandleChip handle={handle} dim={isDone} />}
            {parentTaskName && (
              <Counter testid="canvas-task-parent">
                <CornerDownRight size={10} aria-hidden />
                {parentTaskName}
              </Counter>
            )}
            {freshnessLabel && (
              <Counter testid="canvas-task-freshness">
                <Clock3 size={10} aria-hidden />
                <span
                  title={freshnessAt != null ? formatTime(freshnessAt) : undefined}
                  style={{ fontFamily: "var(--ct-font-mono)", minWidth: "2.2em", textAlign: "right" }}
                >
                  {freshnessLabel}
                </span>
              </Counter>
            )}
            {bodyChars > 0 && <Counter testid="canvas-task-body-count">¶ {Math.max(1, Math.round(bodyChars / 80))}</Counter>}
            {strideCount > 0 && (
              <Counter testid="canvas-task-stride-count">
                {freshColor && <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: freshColor }} />}∿ {strideCount}
              </Counter>
            )}
            {childCount > 0 && (
              <Counter testid="canvas-task-subtask-count">
                <Layers size={10} />
                ⊞ {childDone}/{childCount}
              </Counter>
            )}
            {childCount > 0 && <MiniBar frac={childDone / childCount} />}
          </div>
        </div>

        {/* §35.3 fixed slot: at W2+ the DESCRIPTION box always renders (reveal-by-
            zoom, never click) — body markdown when present, an empty-state otherwise
            so the slot never collapses/shifts. Clamped at W2, full+scroll at W3; the
            stride snake blooms below it. W0/W1 = snake preview pinned to the bottom. */}
        <div className="ct-canvas-fade-in" data-canvas-scroll-body={showFullBody ? undefined : "1"} style={showFullBody
          ? { marginTop: 2, display: "flex", flexDirection: "column", gap: bodyGap, ...revealStyle(true) }
          : { marginTop: 2, overflowY: "hidden", minHeight: 0, display: "grid", gridTemplateRows: bodyRows, gap: bodyGap, ...revealStyle(showBodySlot || strideCount > 0) }}>
          <div data-testid={showBodySlot ? "canvas-task-body" : "canvas-task-body-slot"} data-visible={showBodySlot ? "true" : "false"} data-canvas-selectable className="ct-canvas-text" style={showFullBody
            ? { fontSize: "0.92em", lineHeight: 1.5, minHeight: 0, overflowWrap: "anywhere", ...revealStyle(showBodySlot) }
            : { fontSize: "0.9em", lineHeight: 1.45, minHeight: showBodySlot ? 36 : "1.2em", display: "-webkit-box", WebkitLineClamp: 4, WebkitBoxOrient: "vertical", overflow: "hidden", ...revealStyle(showBodySlot) }}>
              {task.intent && <div data-testid="canvas-task-intent" style={{ marginBottom: 6, fontStyle: "italic" }}>{task.intent}</div>}
              {showFullBody && task.model_rationale && (
                <div data-testid="canvas-task-why" style={{ marginBottom: 6, borderLeft: "2px solid var(--ct-border-subtle)", paddingLeft: 8 }}>
                  <span style={{ fontWeight: 600, color: "var(--ct-border-focus)" }}>Why this model? </span>
                  {task.model_rationale}
                </div>
              )}
              <div data-testid={showFullBody ? "canvas-task-full-body" : "canvas-task-body-preview"}>
                {bodyChars > 0 ? (
                  <RichMarkdown text={task.body} />
                ) : showBodySlot && hasFields ? (
                  <div data-testid="canvas-task-fields">
                    {acItems.length > 0 && (
                      <div data-testid="canvas-task-acceptance" style={{ marginBottom: testPlan ? 8 : 0 }}>
                        <div style={{ fontWeight: 600, color: "var(--ct-border-focus)", marginBottom: 3 }}>Acceptance</div>
                        <ul style={{ margin: 0, paddingLeft: 16 }}>
                          {acItems.map((c, i) => <li key={i} style={{ overflowWrap: "anywhere" }}>{c}</li>)}
                        </ul>
                      </div>
                    )}
                    {testPlan && (
                      <div data-testid="canvas-task-test-plan">
                        <div style={{ fontWeight: 600, color: "var(--ct-border-focus)", marginBottom: 3 }}>Test plan</div>
                        <div style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{testPlan}</div>
                      </div>
                    )}
                  </div>
                ) : (
                  <span style={{ opacity: 0.4, fontStyle: "italic" }}>No description</span>
                )}
              </div>
          </div>
          {showFullBody && hasLaneDetail && lane && <LaneDetail lane={lane} />}
          <div data-testid="canvas-task-strides-slot" data-visible={showStrides ? "true" : "false"} style={{ minHeight: 0, overflow: showFullBody ? "visible" : "hidden", ...revealStyle(showStrides) }}>
            {strideCount > 0 && <StrideSnake milestones={task.milestones} colors={stepColors} world={world} width={innerW} onCopyLink={showFullBody ? onCopyStrideLink : undefined} onFocusStride={showFullBody ? onFocusStride : undefined} />}
          </div>
        </div>
      </div>
    </div>
  );

  // §34 superseded: world geometry stays under the single camera transform; the
  // per-card zoom/counter-scale wrapper reflowed internals and drifted at zoom steps.
  return card(style);
}

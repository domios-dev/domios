import type { CSSProperties } from "react";
import { CheckCircle2, Clock3, ExternalLink, Layers, Link2, TriangleAlert, Unplug } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import { substrateV2Enabled } from "../substrateFlag";
import { atLeast, DEEP_DIVE_CRISP_SCALE, type World } from "../worlds";
import type { DriftItem, DriftKind, TagFacet, Task, TaskStatus } from "../types";
import { resolveTags } from "../tagModel";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";
import { formatTaskFreshness, taskFreshnessAt } from "./taskFreshness";
import { StrideSnake } from "./StrideSnake";
import { formatTime } from "../../../lib/formatters";

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
  if (status === "in_review") {
    return { ring: "0 0 0 1.5px color-mix(in srgb, rgba(245, 158, 11, 0.92) 78%, transparent)", pulse: false, warnBorder: false, recessed: false };
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

function Chip({ testid, mono, accent, children }: { testid: string; mono?: boolean; accent?: string; children: React.ReactNode }) {
  return (
    <span
      data-testid={testid}
      style={{
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
      {children}
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
  stepColors?: Map<string, string>;
  drift?: DriftItem;
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
}

export function TaskCard({
  task,
  tagFacets,
  childCount = 0,
  childDone = 0,
  stepColors,
  drift,
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
}: TaskCardProps) {
  const badgeColor = TASK_STATUS_COLORS[task.status];
  const displayStatus = task.status_label?.trim() || statusLabel(task.status);
  const chrome = statusChrome(task.status, world);
  const shelf = shelfOf(task.status);
  const showTitle = atLeast(world, "W1");
  const showStructure = atLeast(world, "W2");
  const showBodySlot = atLeast(world, "W2");
  const showFullBody = atLeast(world, "W3");
  const showDeepDive = showFullBody && crispScale >= DEEP_DIVE_CRISP_SCALE;
  const innerW = (typeof style.width === "number" ? style.width : 260) - 26;

  const strideCount = task.milestones.length;
  const bodyChars = task.body.trim().length;
  const digest = digestMarkdown(task.intent?.trim() ? task.intent! : task.body, 130);
  const freshKind = strideCount ? [...task.milestones].sort((a, b) => b.ts - a.ts)[0].kind : null;
  const freshColor = freshKind ? (stepColors?.get(freshKind) ?? "var(--ct-border-focus)") : null;
  const titleText = showTitle ? task.name : (task.handle?.trim() || task.name);
  const resolved = resolveTags(task.tags, tagFacets);
  const tileTags = !showBodySlot && (resolved.primary || resolved.topics.length > 0);
  const topicTile = resolved.topics.slice(0, 2);
  const freshnessAt = taskFreshnessAt(task);
  const freshnessLabel = freshnessAt != null ? formatTaskFreshness(freshnessAt) : "";
  const contentRows = showBodySlot ? "auto 20px 0 minmax(36px, 1fr)" : "auto 26px 42px minmax(0, 1fr)";
  const contentGap = showBodySlot ? 4 : 7;
  const contentPadding = showBodySlot ? "8px 10px" : "10px 12px";
  const bodyRows = showFullBody
    ? (showDeepDive ? "auto minmax(0, 1fr)" : "minmax(36px, 0.8fr) minmax(28px, 1fr)")
    : (showBodySlot ? "minmax(36px, 1fr) 0" : "minmax(36px, 0.8fr) minmax(28px, 1fr)");
  const bodyGap = showFullBody || !showBodySlot ? 10 : 0;

  // chrome overrides win over the box geometry (§32.3 surface chrome by shelf).
  const chromeOverrides: CSSProperties = {
    ...(chrome.recessed ? { background: "color-mix(in srgb, var(--ct-fg) 4%, var(--ct-bg))", boxShadow: "inset 0 1px 0 color-mix(in srgb, var(--ct-fg) 6%, transparent)" } : null),
    ...(chrome.ring ? { boxShadow: chrome.ring } : null),
    ...(chrome.warnBorder ? { borderLeft: "3px solid var(--ct-error, #ef8282)" } : null),
    ...(!chrome.warnBorder && resolved.primary?.color ? { borderLeftWidth: 3, borderLeftStyle: "solid", borderLeftColor: resolved.primary.color } : null),
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
      {showBodySlot && onCopyLink && (
        <button type="button" data-testid="canvas-task-copy-link" title="Copy task link" onClick={(e) => { e.stopPropagation(); onCopyLink(task.id); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
          <Link2 size={11} />
        </button>
      )}
    </div>
  );

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
      <div style={{ flex: 1, display: "grid", gridTemplateRows: contentRows, gap: contentGap, padding: contentPadding, minWidth: 0, overflow: "hidden" }}>
        {/* Header — present at every band (the navigation layer). Solid status chip. */}
        <div style={{ display: "flex", alignItems: "flex-start", gap: 8, minWidth: 0 }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", alignItems: "center", gap: 6, minWidth: 0 }}>
              <span data-testid="canvas-task-name" data-visible={showTitle ? "true" : "false"} data-task-name-full={task.name} data-canvas-selectable className="ct-canvas-text" style={{ minWidth: 0, maxWidth: "100%", fontWeight: shelf === "done" ? 600 : 700, fontSize: shelf === "done" ? "0.98em" : "1.03em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", ...revealStyle(showTitle) }}>
                {titleText}
              </span>
              <div style={{ display: "inline-flex", alignItems: "center", gap: 4, flexShrink: 0 }}>
                <StatusChip color={badgeColor} label={displayStatus} solid />
                <DriftBadge drift={drift} />
              </div>
            </div>
          </div>
          {actions}
        </div>

        <div data-testid="canvas-task-meta-slot" style={{ display: "flex", flexWrap: "wrap", alignContent: "flex-start", gap: 6, minHeight: 0, overflow: "hidden", ...revealStyle(showStructure) }}>
          {task.type && <Chip testid="canvas-task-type">{task.type}</Chip>}
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

          <div data-testid="canvas-task-counters" style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", ...revealStyle(bodyChars > 0 || strideCount > 0 || childCount > 0 || freshnessLabel.length > 0) }}>
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
            {childCount > 0 && <Counter testid="canvas-task-subtask-count"><Layers size={10} />⊞ {childCount}</Counter>}
            {childCount > 0 && <MiniBar frac={childDone / childCount} />}
          </div>
        </div>

        {/* §35.3 fixed slot: at W2+ the DESCRIPTION box always renders (reveal-by-
            zoom, never click) — body markdown when present, an empty-state otherwise
            so the slot never collapses/shifts. Clamped at W2, full+scroll at W3; the
            stride snake blooms below it. W0/W1 = snake preview pinned to the bottom. */}
        <div className="ct-canvas-scroll ct-canvas-fade-in" data-canvas-scroll-body="1" style={{ marginTop: 2, overflowY: showFullBody ? "auto" : "hidden", minHeight: 0, display: "grid", gridTemplateRows: bodyRows, gap: bodyGap, ...revealStyle(showBodySlot || strideCount > 0) }}>
          <div data-testid={showBodySlot ? "canvas-task-body" : "canvas-task-body-slot"} data-visible={showBodySlot ? "true" : "false"} data-canvas-selectable className="ct-canvas-text" style={showFullBody
            ? { fontSize: "0.92em", lineHeight: 1.5, minHeight: 0, overflowY: "auto", overflowWrap: "anywhere", ...revealStyle(showBodySlot) }
            : { fontSize: "0.9em", lineHeight: 1.45, minHeight: showBodySlot ? 36 : "1.2em", display: "-webkit-box", WebkitLineClamp: 4, WebkitBoxOrient: "vertical", overflow: "hidden", ...revealStyle(showBodySlot) }}>
              {task.intent && <div data-testid="canvas-task-intent" style={{ marginBottom: 6, fontStyle: "italic" }}>{task.intent}</div>}
              {showFullBody && task.model_rationale && (
                <div data-testid="canvas-task-why" style={{ marginBottom: 6, borderLeft: "2px solid var(--ct-border-subtle)", paddingLeft: 8 }}>
                  <span style={{ fontWeight: 600, color: "var(--ct-border-focus)" }}>Why this model? </span>
                  {task.model_rationale}
                </div>
              )}
              <div data-testid={showFullBody ? "canvas-task-full-body" : "canvas-task-body-preview"}>
                {bodyChars > 0 ? <RichMarkdown text={task.body} /> : <span style={{ opacity: 0.4, fontStyle: "italic" }}>No description</span>}
              </div>
          </div>
          <div data-testid="canvas-task-strides-slot" data-visible={showFullBody || (!showStructure && strideCount > 0) ? "true" : "false"} style={{ minHeight: 0, overflow: showFullBody ? "visible" : "hidden", ...revealStyle(showFullBody || (!showStructure && strideCount > 0)) }}>
            {strideCount > 0 && <StrideSnake milestones={task.milestones} colors={stepColors} world={world} width={innerW} onCopyLink={showFullBody ? onCopyStrideLink : undefined} onFocusStride={showFullBody ? onFocusStride : undefined} />}
          </div>
        </div>
      </div>
    </div>
  );

  if (substrateV2Enabled()) return card(style);
  // §34: birds-eye / 1× → single layer, GPU upscale is imperceptible while small.
  if (crispScale <= 1) return card(style);
  // Readable band → zoom to display px (crisp reflow incl. body + stride snake) then
  // counter-scale back to world size. transform-origin 0,0 keeps footprint identity.
  return (
    <div style={{ ...style, transition: SLOT_TRANSITION }}>
      <div className="ct-canvas-crisp" style={{ width: "100%", height: "100%", zoom: crispScale, transform: `scale(${1 / crispScale})`, transformOrigin: "0 0" }}>
        {card({ width: "100%", height: "100%" })}
      </div>
    </div>
  );
}

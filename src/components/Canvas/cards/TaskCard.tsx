import { useState, type CSSProperties } from "react";
import { Clock3, CornerDownRight, ExternalLink, GitBranch, Layers, Link2, Maximize2, UserRound, Users } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../settings";
import { atLeast, DEEP_DIVE_CRISP_SCALE, readingRootFontPx, World } from "../worlds";
import { TaskPriority, TaskStatus, type DriftItem, type TagFacet, type Task } from "../types";
import { resolveTags } from "../tagModel";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";
import { formatTaskFreshness, taskFreshnessAt } from "./taskFreshness";
import { StrideSnake } from "./StrideSnake";
import { zoneColor } from "../layout/sectionZones";
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
import { cardStyle, statusLabel, Shelf, shelfOf, statusChrome, StatusChip, Counter, MiniBar, Chip, PriorityTag, presenceTooltip, PresenceCluster, HandleChip, LaneDetail, iconBtn, revealStyle, DriftBadge } from "./taskCardParts";

export interface TaskCardProps extends CanvasCardStyleProps {
  task: Task;
  tagFacets?: TagFacet[];
  childCount?: number;
  childDone?: number;
  parentTaskName?: string | null;
  stepColors?: Map<string, string>;
  drift?: DriftItem;
  presence?: TaskPresence | null;
  /** Card surface tint = the task's REGION (its main section; subtasks inherit parent's). */
  regionColor?: string;
  world: World;
  /** §34 crisp text: >1 → re-rasterize at on-screen size (zoom + counter-scale). */
  crispScale?: number;
  onOpenPane?: (paneId: string) => void;
  onHover?: (taskId: string | null) => void;
  /** Expand affordance → open the full READ detail (routed to the existing focus/read path). */
  onExpand?: (taskId: string) => void;
  onCopyLink?: (taskId: string) => void;
  onCopyStrideLink?: (milestoneId: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  tagFilterDimmed?: boolean;
  tagFilterHighlighted?: boolean;
  suppressRichContent?: boolean;
  richOverlay?: boolean;
  /** Render the FULL read content (fullscreen detail surface). On the map this is
   *  false → the card is a schematic index card at every zoom band. */
  detail?: boolean;
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
  regionColor,
  style,
  world,
  crispScale = 1,
  onOpenPane,
  onHover,
  onExpand,
  onCopyLink,
  onCopyStrideLink,
  onFocusStride,
  tagFilterDimmed = false,
  tagFilterHighlighted = false,
  suppressRichContent = false,
  richOverlay = false,
  detail = false,
}: TaskCardProps) {
  const [hovered, setHovered] = useState(false);
  // The READ content renders ONLY in the detail surface (expand) or the substrate
  // rich overlay — never on the map. So map cards stay schematic at EVERY zoom band
  // (the deep-zoom compositor win: far less scaled DOM text to rasterize).
  const isDetail = detail || richOverlay;
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
  const showBodySlot = isDetail && !budgetRich;
  const showFullBody = isDetail && !budgetRich;
  const showDeepDive = showFullBody && crispScale >= DEEP_DIVE_CRISP_SCALE;
  const innerW = (typeof style.width === "number" ? style.width : 260) - 26;

  // §35 reading-size ladder: hold the card's text at a comfortable on-screen size at
  // deep zoom (content scrolls, glyphs never balloon) instead of a fixed-13px world
  // root that paints 60px titles at 5×. crispScale ≈ cameraScale at W2+ (1 below).
  const cardFontPx = readingRootFontPx(world, crispScale);

  const strideCount = task.milestones.length;
  const bodyChars = task.body.trim().length;
  const acItems = Array.isArray(task.acceptance_criteria) ? task.acceptance_criteria.filter((c) => c && c.trim()) : [];
  const testPlan = typeof task.test_plan === "string" ? task.test_plan.trim() : "";
  const correctness = typeof task.correctness === "string" ? task.correctness.trim() : "";
  const taskLinks = [
    ...(Array.isArray(task.links) ? task.links : []),
    ...task.milestones.flatMap((m) => m.links),
  ].map((l) => l?.trim()).filter((l): l is string => !!l);
  const hasFields = acItems.length > 0 || testPlan.length > 0 || correctness.length > 0 || taskLinks.length > 0;
  const digest = digestMarkdown(task.intent?.trim() ? task.intent! : task.body, 130);
  const freshKind = strideCount ? [...task.milestones].sort((a, b) => b.ts - a.ts)[0].kind : null;
  const freshColor = freshKind ? (stepColors?.get(freshKind) ?? "var(--ct-border-focus)") : null;
  const titleText = task.name;
  // The worlds promise: dive in → read everything. Title is the card's identity, so
  // the FULL title reads from W1 up (footprint reserves the height for it) — never
  // mid-word clipped, wraps to as many lines as it needs. Only the far W0 mass-glance
  // clamps to a 2-line ellipsis, where the chip is the signal and the title is a cue.
  const nameClamp = showTitle ? null : 2;
  const resolved = resolveTags(task.tags, tagFacets);
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

  // The card's SURFACE COLOUR is its zone — the hue of its MAIN (first) section, the
  // same for every card in that section. NOT a tag/status/priority colour, NOT a faint
  // wash: the fill itself tells you which area the task lives in. Status/priority are
  // carried by the chips. No left-only rail (a one-sided border reads as broken).
  const sectionTint = regionColor ?? (task.section_ids[0] ? zoneColor(task.section_ids[0]) : undefined);
  const zoneBg = sectionTint
    ? `color-mix(in srgb, ${sectionTint} 16%, var(--ct-bg))`
    : chrome.recessed
      ? "color-mix(in srgb, var(--ct-fg) 4%, var(--ct-bg))"
      : undefined;

  const chromeOverrides: CSSProperties = {
    ...(zoneBg ? { background: zoneBg } : null),
    ...(chrome.recessed ? { boxShadow: "inset 0 1px 0 color-mix(in srgb, var(--ct-fg) 6%, transparent)" } : null),
    ...(chrome.ring ? { boxShadow: chrome.ring } : null),
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
          <ExternalLink size="0.85em" />
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
          <Layers size="0.85em" />
        </button>
      )}
      {showBodySlot && onCopyLink && (
        <button type="button" data-testid="canvas-task-copy-link" title="Copy task link" onClick={(e) => { e.stopPropagation(); onCopyLink(task.id); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
          <Link2 size="0.85em" />
        </button>
      )}
    </div>
  );

  // §header (§35): the TITLE is the card's identity and LEADS the card — top line,
  // full width. Status + meta tags sit BELOW it (headline → labels), never above.
  // At the glance bands the title is a touch smaller and never breaks mid-word (a
  // narrow subtask cell showed "Mileston e…"); at the reading bands it grows and
  // wraps fully (the footprint reserves the height). Only W0 keeps a 2-line cue.
  const titleFontEm = showStructure ? (isDone ? 0.98 : 1.03) : (isDone ? 0.86 : 0.92);
  const nameSpan = (
    <span data-testid="canvas-task-name" data-visible="true" data-task-name-full={task.name} data-canvas-selectable className="ct-canvas-text" style={{ minWidth: 0, maxWidth: "100%", fontWeight: shelf === Shelf.Done ? 600 : 700, fontSize: `${titleFontEm}em`, lineHeight: 1.25, overflowWrap: showStructure ? "break-word" : "normal", wordBreak: "normal", ...(nameClamp != null ? { display: "-webkit-box", WebkitLineClamp: nameClamp, WebkitBoxOrient: "vertical", overflow: "hidden" } : { display: "block" }) }}>
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
  // EMERGENCE (north-star #2): presence lives in the HEADER (never collapses, every
  // band) so "who is here" is glanceable — at W0 a COMPACT cue (agent circle + lead
  // handle only) so the far board already shows where the swarm is working (§32.2
  // W0 identity glance); W1+ adds the branch + activity as the card earns pixels.
  const presenceEl = presence
    ? <PresenceCluster presence={presence} live={task.status === TaskStatus.InProgress} showActivity={showStructure} compact={!showTitle} />
    : null;
  const live = task.status === TaskStatus.InProgress;

  // UX-1 SCHEMATIC MAP FACE (W0-W2): the card is a glanceable index card — fitting
  // title (≤2 lines, never clipped) + an optional one-line description + a tight
  // glyph row (status · priority · who's-here · subtasks). Full body / criteria /
  // strides / links / lane live in the READ surface (expand → W3, until UX-2 builds
  // the dedicated fullscreen detail). Far less scaled DOM than the old in-card reveal.
  const descText = (task.intent?.trim() || digest || "").trim();
  const expandBtn = onExpand && showTitle && !isDetail ? (
    <button
      type="button"
      data-testid="canvas-task-expand"
      title="Expand — open full detail"
      aria-label="Expand task detail"
      onClick={(e) => { e.stopPropagation(); onExpand(task.id); }}
      onMouseDown={(e) => e.stopPropagation()}
      onFocus={() => setHovered(true)}
      onBlur={() => setHovered(false)}
      style={{ position: "absolute", top: 6, right: 6, zIndex: 2, display: "flex", alignItems: "center", justifyContent: "center", width: 20, height: 20, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", opacity: hovered ? 0.95 : 0, transition: "opacity 0.12s ease", pointerEvents: "auto" }}
    >
      <Maximize2 size="0.82em" />
    </button>
  ) : null;
  const schematicFace = (
    <div data-testid="canvas-task-schematic" style={{ flex: 1, display: "flex", flexDirection: "column", gap: 5, padding: showTitle ? "9px 11px" : "8px 10px", minWidth: 0, overflow: "hidden" }}>
      <span
        data-testid="canvas-task-name"
        data-visible="true"
        data-task-name-full={task.name}
        data-canvas-selectable
        className="ct-canvas-text"
        style={{ minWidth: 0, maxWidth: "100%", fontWeight: isDone ? 600 : 700, fontSize: `${titleFontEm}em`, lineHeight: 1.22, overflowWrap: "break-word", wordBreak: "normal", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}
      >
        {titleText}
      </span>
      {showTitle && descText && (
        <span
          data-testid="canvas-task-desc"
          data-canvas-selectable
          className="ct-canvas-text"
          style={{ minWidth: 0, maxWidth: "100%", fontSize: "0.82em", lineHeight: 1.35, color: "color-mix(in srgb, var(--ct-fg) 70%, transparent)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
        >
          {descText}
        </span>
      )}
      <div data-testid="canvas-task-glyphs" style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: 6, marginTop: "auto" }}>
        <StatusChip color={badgeColor} label={displayStatus} solid />
        {task.priority && <PriorityTag priority={task.priority} dim={isDone} />}
        {drift && <DriftBadge drift={drift} />}
        {presence && presence.agents.length > 0 && (
          <span data-testid="canvas-task-presence" data-pane-id={presence.paneId} data-agent-count={presence.agents.length} title={presenceTooltip(presence)} style={{ display: "inline-flex", alignItems: "center" }}>
            <AgentStack agents={presence.agents} size={showStructure ? 18 : 15} live={live} />
          </span>
        )}
        {childCount > 0 && (
          <span data-testid="canvas-task-subtask-count" style={{ display: "inline-flex", alignItems: "center", gap: 3, fontSize: "0.7em", fontWeight: 600, opacity: 0.78 }}>
            <Layers size="0.78em" aria-hidden />⊞ {childDone}/{childCount}
          </span>
        )}
      </div>
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
      onMouseEnter={() => { setHovered(true); onHover?.(task.id); }}
      onMouseLeave={() => { setHovered(false); onHover?.(null); }}
      style={{ ...cardStyle, fontSize: cardFontPx, ...boxStyle, ...chromeOverrides, ...(tagFilterDimmed ? { opacity: 0.22, pointerEvents: "none" as const } : {}) }}
    >
      {expandBtn}
      {!isDetail ? schematicFace : (
      <div
        className={showFullBody ? "ct-canvas-scroll" : undefined}
        data-canvas-scroll-body={showFullBody ? "1" : undefined}
        style={showFullBody
          ? { flex: 1, display: "flex", flexDirection: "column", gap: contentGap, padding: contentPadding, minWidth: 0, overflowY: "auto" }
          : { flex: 1, display: "grid", gridTemplateRows: contentRows, gridTemplateColumns: "minmax(0, 1fr)", gap: contentGap, padding: contentPadding, minWidth: 0, overflow: "hidden" }}
      >
        {/* Header (§35) — TITLE leads on the top line (with node actions on its
            right at W2+), status + presence drop BELOW it. One layout at every band;
            actions are empty at the glance bands. Title is the identity, never a
            label sitting above the name. */}
        <div style={{ display: "flex", flexDirection: "column", gap: 5, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 6, minWidth: 0 }}>
            <div style={{ flex: 1, minWidth: 0 }}>{nameSpan}</div>
            {actions}
          </div>
          {statusCluster}
          {presenceEl}
        </div>

        <div data-testid="canvas-task-meta-slot" style={{ display: "flex", flexWrap: "wrap", alignContent: "flex-start", gap: 6, minHeight: 0, maxHeight: showFullBody ? undefined : 44, overflow: "hidden", ...revealStyle(showStructure) }}>
          {metaPriority && task.priority && <span style={{ display: "inline-flex", alignItems: "center" }}><PriorityTag priority={task.priority} dim={isDone} /></span>}
          {handle && <HandleChip handle={handle} dim={isDone} />}
          {task.type && <Chip testid="canvas-task-type">{task.type}</Chip>}
          {role && <Chip testid="canvas-task-role" accent="rgba(96,165,250,0.95)" icon={<UserRound size="0.85em" aria-hidden />} max={150}>{role}</Chip>}
          {branch && !presence?.branch && <Chip testid="canvas-task-branch" mono icon={<GitBranch size="0.85em" aria-hidden />} max={170}>{branch}</Chip>}
          {peer && !presence?.agents.some((a) => a.isPeer) && <Chip testid="canvas-task-peer" icon={<Users size="0.85em" aria-hidden />} max={140}>{peer}</Chip>}
          {group && <Chip testid="canvas-task-group" max={140}>{group}</Chip>}
          {task.model && <Chip testid="canvas-task-model" mono>{task.model}</Chip>}
          {resolved.facets.map((facet) => (
            <Chip key={facet.tag} testid="canvas-task-tag-facet" accent={facet.color ?? undefined}>{facet.label}</Chip>
          ))}
          {resolved.topics.map((topic) => <Chip key={topic} testid="canvas-task-tag">{topic}</Chip>)}
        </div>

        <div data-testid="canvas-task-summary-slot" style={{ minHeight: 0, overflow: "hidden" }}>
          <div data-testid="canvas-task-counters" style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", ...revealStyle(bodyChars > 0 || strideCount > 0 || childCount > 0 || freshnessLabel.length > 0 || !!parentTaskName || (!!handle && showTitle)) }}>
            {handle && showTitle && <HandleChip handle={handle} dim={isDone} />}
            {parentTaskName && (
              <Counter testid="canvas-task-parent">
                <CornerDownRight size="0.78em" aria-hidden />
                {parentTaskName}
              </Counter>
            )}
            {freshnessLabel && (
              <Counter testid="canvas-task-freshness">
                <Clock3 size="0.78em" aria-hidden />
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
                <Layers size="0.78em" />
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
                ) : (
                  <span style={{ opacity: 0.4, fontStyle: "italic" }}>No description</span>
                )}
              </div>
          </div>
          {/* READ-detail structured fields (UX-2/UX-3): acceptance · test plan ·
              correctness · links — always shown in the detail surface, below the body. */}
          {showFullBody && hasFields && (
            <div data-testid="canvas-task-fields" style={{ display: "flex", flexDirection: "column", gap: 10, fontSize: "0.92em", lineHeight: 1.5 }}>
              {acItems.length > 0 && (
                <div data-testid="canvas-task-acceptance">
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
              {correctness && (
                <div data-testid="canvas-task-correctness">
                  <div style={{ fontWeight: 600, color: "var(--ct-border-focus)", marginBottom: 3 }}>Correctness</div>
                  <div style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{correctness}</div>
                </div>
              )}
              {taskLinks.length > 0 && (
                <div data-testid="canvas-task-links">
                  <div style={{ fontWeight: 600, color: "var(--ct-border-focus)", marginBottom: 3 }}>Links</div>
                  <ul style={{ margin: 0, paddingLeft: 16 }}>
                    {taskLinks.map((l, i) => <li key={i} style={{ overflowWrap: "anywhere", fontFamily: "var(--ct-font-mono)", fontSize: "0.92em" }}>{l}</li>)}
                  </ul>
                </div>
              )}
            </div>
          )}
          {showFullBody && hasLaneDetail && lane && <LaneDetail lane={lane} />}
          <div data-testid="canvas-task-strides-slot" data-visible={showStrides ? "true" : "false"} style={{ minHeight: 0, overflow: showFullBody ? "visible" : "hidden", ...revealStyle(showStrides) }}>
            {strideCount > 0 && <StrideSnake milestones={task.milestones} colors={stepColors} world={world} width={innerW} onCopyLink={showFullBody ? onCopyStrideLink : undefined} onFocusStride={showFullBody ? onFocusStride : undefined} />}
          </div>
        </div>
      </div>
      )}
    </div>
  );

  // §34 superseded: world geometry stays under the single camera transform; the
  // per-card zoom/counter-scale wrapper reflowed internals and drifted at zoom steps.
  return card(style);
}

import { useRef, type CSSProperties } from "react";
import { Archive, FileText, Maximize2 } from "lucide-react";
import type { Plan, TaskStatus } from "../types";
import type { PlanWorkspaceScope } from "../planWorkspaceScope";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { atLeast, type World } from "../worlds";
import { sectionRows } from "../sectionRows";
import { ownerName } from "../owner";
import type { SectionNavArgs } from "../nodeRegistry";
import { planCoverageFraction } from "./coverage";
import type { CanvasCardStyleProps } from "./types";
import {
  CoverageBar,
  GLYPH,
  HEARTBEAT_ACCENT,
  STATUS_ORDER,
  StatusChip,
  SwarmBar,
  frameStyle,
  iconBtn,
} from "./sectionNode/shared";
import { LinkageStrip } from "./sectionNode/LinkageStrip";
import { VocabularyLegend } from "./sectionNode/VocabularyLegend";
import { HeartbeatStrip } from "./sectionNode/HeartbeatStrip";
import { LoopsStrip } from "./sectionNode/LoopsStrip";
import { SwarmPanel } from "./sectionNode/SwarmPanel";
import { SectionRowView } from "./sectionNode/SectionRowView";
import type { SpecRef } from "./sectionNode/SpecRefBubbles";

const S = DEFAULT_CANVAS_SETTINGS;

export interface SectionNodeProps extends CanvasCardStyleProps {
  variant: "plan" | "spec";
  plan: Plan;
  world: World;
  driftCount?: number;
  workspaceScope?: PlanWorkspaceScope;
  /** §34 crisp text: >1 → re-rasterize at on-screen size (zoom + counter-scale). */
  crispScale?: number;
  onArchive?: () => void;
  onFitNode?: () => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  suppressRichContent?: boolean;
  richOverlay?: boolean;
  /** spec_section_id → title, for resolving per-section spec-reference bubble labels. */
  specTitleById?: ReadonlyMap<string, string>;
}

export function SectionNode({ variant, plan, style, world, driftCount = 0, workspaceScope, onArchive, onFitNode, onSectionClick, onSectionFrame, suppressRichContent = false, richOverlay = false, specTitleById }: SectionNodeProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const owner = ownerName(plan.owner);
  const isPlan = variant === "plan";
  const testid = isPlan ? "canvas-plan-node" : "canvas-spec-node";
  const participantTitle = workspaceScope?.participantWorkspaceLabels.length
    ? `Participating workspaces: ${workspaceScope.participantWorkspaceLabels.join(", ")}`
    : undefined;
  const showRows = atLeast(world, "W1");
  const budgetRich = suppressRichContent && !richOverlay;
  const showBody = atLeast(world, "W2") && !budgetRich; // the reveal band (§26.2)
  const scroll = showBody;

  const counts = { uncovered: 0, progress: 0, done: 0 };
  for (const r of rows) counts[r.glyph]++;

  // Heartbeat/loops are PLAN-only live state, rendered only when present (§render-
  // when-present). Heartbeat gates per-band so a rules/learnings-only beat never
  // shows a hollow strip below its reveal band.
  const hb = plan.heartbeat;
  const hbFocus = hb?.current_focus?.trim() ?? "";
  const hbLanes = (hb?.active_lanes ?? []).filter((l) => l.trim());
  const hbRules = (hb?.rules ?? []).filter((r) => r.trim());
  const hbLearnings = (hb?.learnings ?? []).filter((l) => l.text?.trim());
  const showHeartbeat = isPlan && !!hb && (
    !!hbFocus ||
    (atLeast(world, "W1") && hbLanes.length > 0) ||
    (atLeast(world, "W2") && hbRules.length > 0) ||
    (atLeast(world, "W3") && hbLearnings.length > 0)
  );
  const loops = plan.loops ?? [];
  const showLoops = isPlan && loops.length > 0;

  // Linkage = the plan's edges to other efforts (spec binding + lineage); vocab =
  // its declared milestone-kind / tag-facet legends. Both PLAN-only, render-when-
  // present. Linkage is glanceable (all bands); vocab reveals inside the body at W2+.
  const hasSpecLink = !!plan.primary_spec_id?.trim() || (plan.spec_links?.length ?? 0) > 0;
  const hasLineage = (plan.lineage ?? []).some((l) => l.to?.trim());
  const showLinkage = isPlan && (hasSpecLink || hasLineage);
  const hasVocab = (plan.milestone_kinds?.length ?? 0) > 0 || (plan.tag_facets ?? []).some((f) => f.values.length > 0);
  const showVocab = isPlan && showBody && hasVocab;

  // §swarm: the plan's tasks fill the body as live collective state. When present
  // the section index sits at natural height and the swarm panel takes the slack —
  // killing the empty void a sparse section list used to leave (north-star: never
  // куцо). PLAN-only; spec keeps its prose-driven layout untouched.
  const tasks = plan.tasks ?? [];
  const showSwarm = isPlan && tasks.length > 0;
  const liveTaskCount = tasks.reduce((n, t) => n + (t.status === "in_progress" ? 1 : 0), 0);

  // §plan-as-doc: resolve, per plan section, the spec section(s) it derives from
  // (spec_links plan_section_id → spec_section_id), titled from the linked spec.
  // These render as inline spec-reference bubbles — they REPLACE node-to-node edges.
  const specRefsBySection = (() => {
    if (!isPlan) return undefined;
    const links = plan.spec_links ?? [];
    if (!links.length) return undefined;
    const m = new Map<string, SpecRef[]>();
    const seen = new Map<string, Set<string>>();
    for (const l of links) {
      const ps = l.plan_section_id?.trim();
      const ss = l.spec_section_id?.trim();
      if (!ps || !ss) continue;
      let dedup = seen.get(ps);
      if (!dedup) { dedup = new Set(); seen.set(ps, dedup); }
      if (dedup.has(ss)) continue;
      dedup.add(ss);
      const arr = m.get(ps) ?? [];
      arr.push({ id: ss, title: specTitleById?.get(ss) ?? "" });
      m.set(ps, arr);
    }
    return m;
  })();

  const card = (boxStyle: CSSProperties) => (
    <div data-testid={testid} data-plan-id={plan.id} data-plan-status={plan.status} data-world={world} className="ct-canvas-node" style={{ ...frameStyle(variant), ...boxStyle }}>
      {isPlan && <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />}

      <div style={{ height: S.planHeaderHeight - (isPlan ? 3 : 0), boxSizing: "border-box", padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 6, flexShrink: 0 }}>
        {/* §header: title owns the full first line (no inline chip/button competing
            for width); drift + node actions drop to the coverage meta row below —
            same "title is the identity" principle the task cards follow. */}
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          {!isPlan && <FileText size={13} style={{ flexShrink: 0, opacity: 0.7 }} />}
          <span title={plan.title} data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ fontWeight: 700, flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", letterSpacing: "-0.01em" }}>
            {isPlan ? plan.title : "Specification"}
          </span>
        </div>
        {isPlan ? (
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.86em", minWidth: 0 }}>
            <StatusChip status={plan.status} world={world} />
            {owner && (
              <span data-testid="canvas-plan-owner" title={workspaceScope?.ownerWorkspaceLabel ? `${owner} @ ${workspaceScope.ownerWorkspaceLabel}` : owner} style={{ display: "flex", alignItems: "center", gap: 5, maxWidth: 190, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", padding: "1px 8px 1px 6px", borderRadius: 999, background: "var(--ct-bg-selected)", border: "1px solid var(--ct-border-subtle)" }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: "rgba(56,189,248,0.95)", flexShrink: 0 }} />
                {owner}
                {workspaceScope?.ownerWorkspaceLabel && <span style={{ opacity: "var(--ct-text-secondary-opacity)" }}>@ {workspaceScope.ownerWorkspaceLabel}</span>}
              </span>
            )}
            {workspaceScope?.isCrossWorkspace && (
              <span data-testid="canvas-plan-cross-workspace" title={participantTitle} style={{ display: "inline-flex", alignItems: "center", height: 20, padding: "0 7px", borderRadius: 999, fontSize: "0.78em", fontWeight: 800, letterSpacing: "0.01em", color: "rgba(129,140,248,0.98)", background: "color-mix(in srgb, rgba(129,140,248,0.98) 13%, transparent)", border: "1px solid color-mix(in srgb, rgba(129,140,248,0.98) 48%, transparent)", whiteSpace: "nowrap", flexShrink: 0 }}>
                cross-workspace
              </span>
            )}
            <CoverageBar pct={pct} />
            <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>{pct}%</span>
            <span
              data-testid={driftCount > 0 ? "canvas-plan-drift-count" : "canvas-plan-drift-slot"}
              title={driftCount > 0 ? `${driftCount} drift item${driftCount === 1 ? "" : "s"}` : undefined}
              style={{
                minWidth: 48,
                height: 20,
                padding: "0 7px",
                borderRadius: 999,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "0.86em",
                fontWeight: 800,
                color: "rgba(245,158,11,0.98)",
                background: "color-mix(in srgb, rgba(245,158,11,0.98) 13%, transparent)",
                border: "1px solid color-mix(in srgb, rgba(245,158,11,0.98) 50%, transparent)",
                opacity: driftCount > 0 ? 1 : 0,
                visibility: driftCount > 0 ? "visible" : "hidden",
                flexShrink: 0,
              }}
            >
              {driftCount} drift
            </span>
            {onFitNode && (
              <button type="button" data-testid="canvas-plan-fit" title="Focus this plan" onClick={(e) => { e.stopPropagation(); onFitNode(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
                <Maximize2 size={12} />
              </button>
            )}
            {onArchive && (
              <button type="button" data-testid="canvas-plan-archive" title="Archive plan" onClick={(e) => { e.stopPropagation(); onArchive(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
                <Archive size={12} />
              </button>
            )}
          </div>
        ) : (
          <span style={{ fontSize: "0.76em", opacity: "var(--ct-text-tertiary-opacity)" }}>{rows.length} section{rows.length === 1 ? "" : "s"}</span>
        )}
      </div>

      {isPlan && plan.goal?.vector && (
        <div
          data-testid="canvas-plan-goal"
          data-canvas-selectable
          className="ct-canvas-text"
          style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", background: "color-mix(in srgb, var(--ct-border-focus) 9%, transparent)", fontSize: "0.82em", lineHeight: 1.3, display: "flex", gap: 6, alignItems: "flex-start" }}
        >
          <span style={{ flexShrink: 0 }} title="Goal (north-star)">🎯</span>
          <span title={plan.goal.vector} style={{ display: "-webkit-box", WebkitLineClamp: atLeast(world, "W1") ? 3 : 1, WebkitBoxOrient: "vertical", overflow: "hidden", fontWeight: 600, letterSpacing: "-0.01em" }}>
            {plan.goal.vector}
          </span>
        </div>
      )}

      {showLinkage && <LinkageStrip plan={plan} world={world} />}
      {showHeartbeat && hb && <HeartbeatStrip hb={hb} world={world} />}
      {showLoops && <LoopsStrip loops={loops} world={world} />}

      <div
        ref={bodyRef}
        className="ct-canvas-scroll"
        data-testid={isPlan ? "canvas-plan-body" : "canvas-spec-body"}
        data-content={showBody ? "body" : "index"}
        data-canvas-scroll-body={scroll ? "1" : undefined}
        style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflowY: scroll ? "auto" : "hidden" }}
      >
        {rows.length === 0 ? (
          // No sections: the swarm IS the plan's content — render it full, never a
          // bare "N standalone tasks" line in a void.
          showSwarm ? (
            <SwarmPanel tasks={tasks} world={world} />
          ) : (
            <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.9em", opacity: "var(--ct-text-tertiary-opacity)" }}>
              {isPlan ? "no tasks yet" : "no spec sections"}
            </div>
          )
        ) : showRows ? (
          <>
            {showVocab && <VocabularyLegend plan={plan} world={world} />}
            {rows.map((row) => (
              <SectionRowView key={row.section.id} row={row} variant={variant} world={world} planId={plan.id} specRefs={specRefsBySection?.get(row.section.id)} onSectionClick={onSectionClick} onSectionFrame={onSectionFrame} suppressRichContent={budgetRich} stretchEmpty={!showSwarm} />
            ))}
            {showSwarm && <SwarmPanel tasks={tasks} world={world} />}
          </>
        ) : (
          // §32.2 W0: COMPRESSED — top section rows + status counts + swarm pulse,
          // never hollow (north-star: meaningful far glance).
          <div data-testid="canvas-section-compressed" style={{ flex: 1, display: "flex", flexDirection: "column", gap: 5, padding: "10px 10px", justifyContent: "flex-start" }}>
            {rows.slice(0, 3).map((row) => {
              const g = GLYPH[row.glyph];
              return (
                <div key={row.section.id} style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                  <g.Icon size={11} color={g.color} style={{ flexShrink: 0 }} strokeWidth={2.2} />
                  <span style={{ flex: 1, minWidth: 0, fontSize: "0.8em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{row.section.title}</span>
                  {isPlan && <span style={{ fontSize: "0.66em", fontFamily: "var(--ct-font-mono)", opacity: 0.65 }}>{row.taskCount}</span>}
                </div>
              );
            })}
            {rows.length > 3 && <span style={{ fontSize: "0.68em", opacity: 0.6, paddingLeft: 17 }}>+{rows.length - 3} more</span>}
            <div data-testid="canvas-section-counts" style={{ display: "flex", gap: 8, marginTop: 4, fontSize: "0.68em", fontWeight: 600 }}>
              {counts.progress > 0 && <span style={{ color: GLYPH.progress.color }}>● {counts.progress} active</span>}
              {counts.uncovered > 0 && <span style={{ color: GLYPH.uncovered.color }}>▲ {counts.uncovered} uncovered</span>}
              {counts.done > 0 && <span style={{ color: GLYPH.done.color }}>✓ {counts.done} done</span>}
            </div>
            {showSwarm && (
              <div data-testid="canvas-plan-swarm-glance" style={{ display: "flex", alignItems: "center", gap: 7, marginTop: 5, minWidth: 0 }}>
                <span style={{ flex: 1, minWidth: 0, display: "flex" }}>
                  <SwarmBar counts={tasks.reduce((c, t) => { c[t.status] = (c[t.status] ?? 0) + 1; return c; }, { in_progress: 0, in_review: 0, blocked: 0, pending: 0, done: 0, cancelled: 0 } as Record<TaskStatus, number>)} order={STATUS_ORDER} total={tasks.length} />
                </span>
                <span style={{ flexShrink: 0, fontSize: "0.66em", fontWeight: 700, color: liveTaskCount > 0 ? HEARTBEAT_ACCENT : "var(--ct-fg)", opacity: liveTaskCount > 0 ? 1 : 0.7, whiteSpace: "nowrap" }}>
                  {liveTaskCount > 0 ? `${liveTaskCount} live` : `${tasks.length} task${tasks.length === 1 ? "" : "s"}`}
                </span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );

  // §34 superseded: world geometry stays under the single camera transform; the
  // per-card zoom/counter-scale wrapper reflowed internals and drifted at zoom steps.
  return card(style);
}

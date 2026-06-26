import { useRef, type CSSProperties } from "react";
import { Activity, Archive, Check, CircleDot, Copy, Crosshair, FileText, GitBranch, Maximize2, Repeat, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { LoopSpec, LoopStatus, OrchestratorHeartbeat, Plan } from "../types";
import type { PlanWorkspaceScope } from "../planWorkspaceScope";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { atLeast, type World } from "../worlds";
import { sectionRows, type SectionRow, type SectionRowGlyph } from "../sectionRows";
import { ownerName } from "../owner";
import type { SectionNavArgs } from "../nodeRegistry";
import { planCoverageFraction } from "./coverage";
import { digestMarkdown } from "./digest";
import type { CanvasCardStyleProps } from "./types";

const S = DEFAULT_CANVAS_SETTINGS;

// §24.3 / §32: SPEC and PLAN are two connected sectioned nodes (variant), each a
// FIXED frame whose CONTENT adapts to the node's appW WORLD band (§26.2):
//   W0 mass     — header + coverage + top-3 section rows + status counts (§32.2 —
//                 COMPRESSED content, never a hollow "N sections" placeholder).
//   W1 structure— all section rows (glyph · title · count · output dot).
//   W2 content  — bodies reveal: SPEC prose markdown, PLAN excerpts + affordances.
//   W3 deep     — full bodies, node-internal scroll. SPEC owns the full prose.
const frameStyle = (variant: "plan" | "spec"): CSSProperties => ({
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-lg)",
  background: variant === "spec" ? "var(--ct-titlebar-bg)" : "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow:
    variant === "plan"
      ? "0 10px 28px -8px rgba(0,0,0,0.45), 0 2px 6px -2px rgba(0,0,0,0.3)"
      : "var(--ct-shadow-sm)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
});

const GLYPH = {
  done: { Icon: Check, color: "rgba(52,211,153,1)", rail: "rgba(52,211,153,0.9)" },
  progress: { Icon: CircleDot, color: "rgba(129,140,248,1)", rail: "rgba(99,102,241,0.95)" },
  uncovered: { Icon: TriangleAlert, color: "rgba(251,191,36,1)", rail: "rgba(251,191,36,1)" },
} as const satisfies Record<SectionRowGlyph, { Icon: typeof Check; color: string; rail: string }>;

// The orchestrator's LIVE pulse, distinct from the static goal (north-star): reuse
// the product's "live presence" green (matches TaskCard's working pill) so a plan
// that is actively being driven reads the same everywhere.
const HEARTBEAT_ACCENT = "rgba(82,180,145,0.95)";

// Loop status maps onto the SAME semantics as section-row glyphs: running = the
// "progress" indigo (+ pulse), done = green, exhausted = amber attention, stopped
// = quiet. No new color vocabulary.
const LOOP_STATUS: Record<LoopStatus, { label: string; color: string; live: boolean }> = {
  running: { label: "running", color: "rgba(129,140,248,1)", live: true },
  done: { label: "done", color: "rgba(52,211,153,1)", live: false },
  exhausted: { label: "exhausted", color: "rgba(251,191,36,1)", live: false },
  stopped: { label: "stopped", color: "var(--ct-fg)", live: false },
};

function iconBtn(): CSSProperties {
  return { display: "flex", alignItems: "center", justifyContent: "center", width: 22, height: 22, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

function smallBtn(): CSSProperties {
  return { display: "inline-flex", alignItems: "center", justifyContent: "center", width: 18, height: 18, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}

function copyText(text: string): void {
  if (typeof navigator !== "undefined" && navigator.clipboard) void navigator.clipboard.writeText(text).catch(() => {});
}

function SectionMarkdownBody({ text, testid }: { text: string; testid: string }) {
  return (
    <div
      data-testid={testid}
      data-renderer="rich-markdown"
      data-canvas-selectable
      className="ct-canvas-fade-in ct-canvas-text"
      style={{ fontSize: "inherit", lineHeight: "inherit", overflowWrap: "anywhere" }}
    >
      <RichMarkdown text={text} />
    </div>
  );
}

function CoverageBar({ pct }: { pct: number }) {
  return (
    <div title={`${pct}% covered`} style={{ flex: 1, height: 6, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
      <div data-testid="canvas-plan-coverage-bar" style={{ width: `${pct}%`, height: "100%", background: pct >= 100 ? "rgba(52,211,153,0.95)" : "var(--ct-border-focus)" }} />
    </div>
  );
}

// §heartbeat: the orchestrator's PRESENT tense — current focus + the lanes it is
// driving right now, with operating rules (W2+) and recent learnings (W3) below.
// The goal strip above is the destination; this strip is the live pulse toward it.
function HeartbeatStrip({ hb, world }: { hb: OrchestratorHeartbeat; world: World }) {
  const focus = hb.current_focus?.trim() ?? "";
  const lanes = (hb.active_lanes ?? []).map((l) => l.trim()).filter(Boolean);
  const rules = atLeast(world, "W2") ? (hb.rules ?? []).map((r) => r.trim()).filter(Boolean) : [];
  const learnings = atLeast(world, "W3") ? (hb.learnings ?? []).filter((l) => l.text?.trim()) : [];
  const showLanes = atLeast(world, "W1") && lanes.length > 0;
  const maxLanes = 5;

  return (
    <div
      data-testid="canvas-plan-heartbeat"
      className="ct-canvas-fade-in"
      style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", background: `color-mix(in srgb, ${HEARTBEAT_ACCENT} 7%, transparent)`, display: "flex", flexDirection: "column", gap: 5 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <span aria-hidden style={{ display: "inline-flex", alignItems: "center", flexShrink: 0, color: HEARTBEAT_ACCENT }}>
          <span className="ct-canvas-heartbeat-dot" style={{ width: 7, height: 7, borderRadius: "50%", background: HEARTBEAT_ACCENT, marginRight: 5 }} />
          <Activity size={12} strokeWidth={2.4} />
        </span>
        <span style={{ fontSize: "0.62em", fontWeight: 800, letterSpacing: "0.14em", textTransform: "uppercase", color: HEARTBEAT_ACCENT, flexShrink: 0 }}>Now</span>
        {focus && (
          <span
            data-testid="canvas-plan-focus"
            title={focus}
            data-canvas-selectable
            className="ct-canvas-text"
            style={{ flex: 1, minWidth: 0, fontSize: "0.82em", fontWeight: 600, lineHeight: 1.3, display: "-webkit-box", WebkitLineClamp: atLeast(world, "W1") ? 2 : 1, WebkitBoxOrient: "vertical", overflow: "hidden" }}
          >
            {focus}
          </span>
        )}
      </div>
      {showLanes && (
        <div data-testid="canvas-plan-lanes" style={{ display: "flex", flexWrap: "wrap", gap: 5, paddingLeft: 1 }}>
          {lanes.slice(0, maxLanes).map((lane) => (
            <span
              key={lane}
              data-testid="canvas-plan-lane"
              title={lane}
              style={{ display: "inline-flex", alignItems: "center", gap: 4, maxWidth: 160, fontSize: "0.72em", fontWeight: 600, padding: "0.05em 0.5em", borderRadius: 999, border: `1px solid color-mix(in srgb, ${HEARTBEAT_ACCENT} 45%, transparent)`, background: `color-mix(in srgb, ${HEARTBEAT_ACCENT} 13%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}
            >
              <GitBranch size={10} aria-hidden style={{ flexShrink: 0, opacity: 0.8 }} />
              <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{lane}</span>
            </span>
          ))}
          {lanes.length > maxLanes && <span style={{ fontSize: "0.7em", opacity: 0.6, alignSelf: "center" }}>+{lanes.length - maxLanes}</span>}
        </div>
      )}
      {rules.length > 0 && (
        <ul data-testid="canvas-plan-rules" style={{ margin: 0, paddingLeft: 16, fontSize: "0.76em", lineHeight: 1.4, opacity: 0.92, display: "flex", flexDirection: "column", gap: 2 }}>
          {rules.slice(0, 4).map((r, i) => (
            <li key={i} style={{ overflowWrap: "anywhere" }}>{r}</li>
          ))}
          {rules.length > 4 && <li style={{ listStyle: "none", opacity: 0.6 }}>+{rules.length - 4} more</li>}
        </ul>
      )}
      {learnings.length > 0 && (
        <div data-testid="canvas-plan-learnings" style={{ display: "flex", flexDirection: "column", gap: 3, marginTop: 1 }}>
          {learnings.slice(-3).reverse().map((l) => (
            <div key={l.id} style={{ display: "flex", gap: 5, fontSize: "0.75em", lineHeight: 1.35 }}>
              <span aria-hidden style={{ flexShrink: 0, opacity: 0.7 }}>💡</span>
              <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>
                {l.tag && <span style={{ fontWeight: 700, opacity: 0.8, marginRight: 4 }}>{l.tag}</span>}
                {l.text}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function LoopPill({ loop, showDetail }: { loop: LoopSpec; showDetail: boolean }) {
  const meta = LOOP_STATUS[loop.status] ?? LOOP_STATUS.stopped;
  const stages = showDetail ? loop.stages.map((s) => s.role || s.key).filter(Boolean) : [];
  const title = [
    `Loop: ${loop.target}`,
    `Status: ${meta.label}`,
    `Iterations: ${loop.count}${loop.budget > 0 ? ` / ${loop.budget}` : ""}`,
    loop.stop_when ? `Stop when: ${loop.stop_when}` : null,
    stages.length ? `Stages: ${stages.join(" › ")}` : null,
  ].filter(Boolean).join("\n");
  return (
    <span
      data-testid="canvas-plan-loop"
      data-loop-status={loop.status}
      title={title}
      style={{ display: "inline-flex", alignItems: "center", gap: 6, maxWidth: showDetail ? 300 : 210, padding: "0.12em 0.55em", borderRadius: 999, fontSize: "0.76em", border: `1px solid color-mix(in srgb, ${meta.color} 45%, transparent)`, background: `color-mix(in srgb, ${meta.color} 12%, transparent)`, color: "var(--ct-fg)", whiteSpace: "nowrap" }}
    >
      <span className={meta.live ? "ct-canvas-heartbeat-dot" : undefined} aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: meta.color, flexShrink: 0 }} />
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", fontWeight: 600 }}>{loop.target}</span>
      {loop.budget > 0 && (
        <span data-testid="canvas-plan-loop-count" style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.92em", opacity: 0.8, flexShrink: 0 }}>
          {loop.count}/{loop.budget}
        </span>
      )}
      {stages.length > 0 && (
        <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", opacity: 0.6, fontSize: "0.9em" }}>{stages.join(" › ")}</span>
      )}
    </span>
  );
}

// §loops: surface the orchestrator's iteration engines. W0 = a one-line count/running
// summary (never a hollow strip); W1+ = a pill per loop (target · count/budget · status
// dot), revealing pipeline stages + stop-when at W2.
function LoopsStrip({ loops, world }: { loops: LoopSpec[]; world: World }) {
  const running = loops.filter((l) => l.status === "running").length;
  const showDetail = atLeast(world, "W2");
  const maxLoops = 4;

  if (!atLeast(world, "W1")) {
    return (
      <div data-testid="canvas-plan-loops" style={{ flexShrink: 0, padding: "5px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", alignItems: "center", gap: 6, fontSize: "0.74em", fontWeight: 600 }}>
        <Repeat size={11} strokeWidth={2.2} style={{ opacity: 0.7, flexShrink: 0 }} aria-hidden />
        <span>{loops.length} loop{loops.length === 1 ? "" : "s"}</span>
        {running > 0 && <span style={{ color: LOOP_STATUS.running.color }}>· {running} running</span>}
      </div>
    );
  }
  return (
    <div data-testid="canvas-plan-loops" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      <Repeat size={12} strokeWidth={2.2} style={{ opacity: 0.6, flexShrink: 0 }} aria-hidden />
      {loops.slice(0, maxLoops).map((loop) => (
        <LoopPill key={loop.id} loop={loop} showDetail={showDetail} />
      ))}
      {loops.length > maxLoops && <span style={{ fontSize: "0.72em", opacity: 0.6, alignSelf: "center" }}>+{loops.length - maxLoops} more</span>}
    </div>
  );
}

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
}

export function SectionNode({ variant, plan, style, world, driftCount = 0, workspaceScope, onArchive, onFitNode, onSectionClick, onSectionFrame, suppressRichContent = false, richOverlay = false }: SectionNodeProps) {
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

  const card = (boxStyle: CSSProperties) => (
    <div data-testid={testid} data-plan-id={plan.id} data-world={world} className="ct-canvas-node" style={{ ...frameStyle(variant), ...boxStyle }}>
      {isPlan && <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />}

      <div style={{ height: S.planHeaderHeight - (isPlan ? 3 : 0), boxSizing: "border-box", padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 6, flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {!isPlan && <FileText size={13} style={{ flexShrink: 0, opacity: 0.7 }} />}
          <span title={plan.title} data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ fontWeight: 700, flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", letterSpacing: "-0.01em" }}>
            {isPlan ? plan.title : "Specification"}
          </span>
          {isPlan && (
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
                fontSize: "0.72em",
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
          )}
          {isPlan && onFitNode && (
            <button type="button" data-testid="canvas-plan-fit" title="Focus this plan" onClick={(e) => { e.stopPropagation(); onFitNode(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
              <Maximize2 size={12} />
            </button>
          )}
          {isPlan && onArchive && (
            <button type="button" data-testid="canvas-plan-archive" title="Archive plan" onClick={(e) => { e.stopPropagation(); onArchive(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn()}>
              <Archive size={12} />
            </button>
          )}
        </div>
        {isPlan ? (
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.86em" }}>
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
          <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.9em", opacity: "var(--ct-text-tertiary-opacity)" }}>
            {isPlan ? `${plan.tasks.length} standalone task${plan.tasks.length === 1 ? "" : "s"}` : "no spec sections"}
          </div>
        ) : showRows ? (
          rows.map((row) => (
            <SectionRowView key={row.section.id} row={row} variant={variant} world={world} planId={plan.id} onSectionClick={onSectionClick} onSectionFrame={onSectionFrame} suppressRichContent={budgetRich} />
          ))
        ) : (
          // §32.2 W0: COMPRESSED — top section rows + status counts, never hollow.
          <div data-testid="canvas-section-compressed" style={{ flex: 1, display: "flex", flexDirection: "column", gap: 3, padding: "6px 8px", justifyContent: "center" }}>
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
          </div>
        )}
      </div>
    </div>
  );

  // §34 superseded: world geometry stays under the single camera transform; the
  // per-card zoom/counter-scale wrapper reflowed internals and drifted at zoom steps.
  return card(style);
}

// One section. PLAN row = index (glyph·title·count·output dot) + a CLAMPED excerpt
// at W2+ (no W2→W3 toggle). SPEC block = title + 1-line abstract at W1, then the
// DEEPEST (unclamped) prose layout the moment it reveals at W2 — §22.1 reserve-at-
// deepest applied to content: W2→W3 changes only rasterization, never geometry.
// §32.7: single-click highlights connected tasks; double-click / target button
// frames them; copy-id yanks the section id. All camera motion is user-initiated.
function SectionRowView({ row, variant, world, planId, onSectionClick, onSectionFrame, suppressRichContent = false }: {
  row: SectionRow; variant: "plan" | "spec"; world: World; planId: string;
  onSectionClick?: (a: SectionNavArgs) => void; onSectionFrame?: (a: SectionNavArgs) => void;
  suppressRichContent?: boolean;
}) {
  const g = GLYPH[row.glyph];
  const isPlan = variant === "plan";
  const hasBody = row.section.body.trim().length > 0;
  const showBody = atLeast(world, "W2") && hasBody && !suppressRichContent;
  const showAbstract = !isPlan && !showBody && atLeast(world, "W1") && hasBody && !suppressRichContent;
  const deep = atLeast(world, "W3");
  const clamp = isPlan && !deep; // §22.1 spec unclamped from W2; PLAN excerpt clamps at W2 but opens FULLY at W3 (deep read)
  const grow = showBody || showAbstract;
  const nav: SectionNavArgs = { planId, sectionId: row.section.id, variant };
  const showActions = atLeast(world, "W2");

  return (
    <div
      data-testid={isPlan ? "canvas-plan-row" : "canvas-spec-block"}
      data-section-id={row.section.id}
      data-row-glyph={row.glyph}
      onClick={onSectionClick ? (e) => { e.stopPropagation(); onSectionClick(nav); } : undefined}
      onDoubleClick={onSectionFrame ? (e) => { e.stopPropagation(); onSectionFrame(nav); } : undefined}
      style={{
        flex: grow ? "0 0 auto" : 1,
        minHeight: 0,
        position: "relative",
        display: "flex",
        flexDirection: "column",
        gap: 4,
        padding: grow ? "8px 12px" : "0 4px 0 8px",
        justifyContent: "center",
        borderLeft: `2px solid ${g.rail}`,
        borderBottom: grow ? "1px solid var(--ct-border-subtle)" : "none",
        overflow: "hidden",
        cursor: onSectionClick ? "pointer" : undefined,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: row.depth * 8 }}>
        <g.Icon size={12} color={g.color} style={{ flexShrink: 0 }} strokeWidth={row.glyph === "uncovered" ? 2.4 : 2.2} />
        <span data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ flex: 1, minWidth: 0, fontSize: "0.84em", fontWeight: row.glyph === "uncovered" ? 600 : 500, ...(deep ? { whiteSpace: "normal", wordBreak: "break-word" } : { whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }) }}>
          {row.section.title}
        </span>
        {isPlan && <span data-testid="canvas-plan-row-count" style={{ flexShrink: 0, fontSize: "0.68em", fontFamily: "var(--ct-font-mono)", fontWeight: 600, opacity: 0.7, paddingRight: showActions ? 0 : 8 }}>{row.taskCount}</span>}
        {showActions && (
          <span style={{ display: "inline-flex", gap: 4, flexShrink: 0 }}>
            {onSectionFrame && (
              <button type="button" data-testid="canvas-section-goto" title={isPlan ? "Frame connected tasks" : "Frame mapped plan/tasks"} onClick={(e) => { e.stopPropagation(); onSectionFrame(nav); }} onMouseDown={(e) => e.stopPropagation()} style={smallBtn()}>
                <Crosshair size={11} />
              </button>
            )}
            <button type="button" data-testid="canvas-section-copy-id" title="Copy section id" onClick={(e) => { e.stopPropagation(); copyText(row.section.id); }} onMouseDown={(e) => e.stopPropagation()} style={smallBtn()}>
              <Copy size={11} />
            </button>
          </span>
        )}
      </div>
      {showAbstract && (
        <div data-testid="canvas-spec-abstract" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.78em", lineHeight: 1.4, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", paddingLeft: row.depth * 8 }}>
          {digestMarkdown(row.section.body, 80)}
        </div>
      )}
      {showBody && (
        <div
          style={clamp
            ? { fontSize: "0.82em", lineHeight: 1.5, paddingLeft: row.depth * 8, display: "-webkit-box", WebkitLineClamp: isPlan ? 4 : 8, WebkitBoxOrient: "vertical", overflow: "hidden" }
            : { fontSize: "0.82em", lineHeight: 1.55, paddingLeft: row.depth * 8 }}
        >
          <SectionMarkdownBody
            text={row.section.body}
            testid={isPlan ? "canvas-plan-section-body" : "canvas-spec-section-body"}
          />
        </div>
      )}
      {/* Per-section anchor dot on the node's right edge: PLAN output → tasks; SPEC → plan row. */}
      <span data-testid={isPlan ? "canvas-plan-output" : "canvas-spec-output"} aria-hidden style={{ position: "absolute", right: -1, top: grow ? 14 : "50%", transform: grow ? "none" : "translateY(-50%)", width: 7, height: 7, borderRadius: "50%", background: g.rail, border: "1px solid var(--ct-bg)" }} />
    </div>
  );
}

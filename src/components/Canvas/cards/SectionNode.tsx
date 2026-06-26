import { useRef, type CSSProperties } from "react";
import { Archive, Check, CircleDot, Copy, Crosshair, FileText, Maximize2, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan } from "../types";
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

export function SectionNode({ variant, plan, style, world, driftCount = 0, workspaceScope, crispScale = 1, onArchive, onFitNode, onSectionClick, onSectionFrame, suppressRichContent = false, richOverlay = false }: SectionNodeProps) {
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

  // §34: birds-eye / 1× → single layer, GPU upscale is imperceptible while small.
  if (crispScale <= 1) return card(style);
  // Readable band → zoom the card to display px (crisp reflow) then counter-scale it
  // back to world size on its own compositing layer. zoom × scale(1/zoom) = 1× world.
  // transform-origin MUST be top-left: default center drifts the box by (k-1)/2·size
  // → footprint appears to move/resize on zoom (§22.1/§37.1 boxiness). 0,0 = identity.
  return (
    <div style={style}>
      <div className="ct-canvas-crisp" style={{ width: "100%", height: "100%", zoom: crispScale, transform: `scale(${1 / crispScale})`, transformOrigin: "0 0" }}>
        {card({ width: "100%", height: "100%" })}
      </div>
    </div>
  );
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
  const clamp = isPlan; // §22.1: spec body is unclamped from W2 on (deepest layout reserved, no W2→W3 reflow)
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
        <span data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ flex: 1, minWidth: 0, fontSize: "0.84em", fontWeight: row.glyph === "uncovered" ? 600 : 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
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

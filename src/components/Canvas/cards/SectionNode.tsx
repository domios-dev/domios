import { useRef, type CSSProperties } from "react";
import { Archive, Check, CircleDot, FileText, Maximize2, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan } from "../types";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { sectionRows, type SectionRow, type SectionRowGlyph } from "../sectionRows";
import { ownerName } from "../owner";
import { planCoverageFraction } from "./coverage";
import type { CanvasCardStyleProps } from "./types";

const S = DEFAULT_CANVAS_SETTINGS;

// §24.3 un-merge: SPEC and PLAN are two connected sectioned nodes (variant). Both
// have a header + a per-section body (evenly distributed so rows align 1:1 for the
// spec↔plan edges) + a right-edge anchor dot per section. PLAN rows are the index
// (title·coverage·[n]·output→tasks); SPEC blocks reveal their prose on zoom (§24.1
// content-LOD). FIXED frame — zoom is camera-only, content adapts inside it.
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
  contentNear?: boolean;
  onArchive?: () => void;
  onFitNode?: () => void;
}

export function SectionNode({ variant, plan, style, contentNear = false, onArchive, onFitNode }: SectionNodeProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const owner = ownerName(plan.owner);
  const isPlan = variant === "plan";
  const testid = isPlan ? "canvas-plan-node" : "canvas-spec-node";

  return (
    <div data-testid={testid} data-plan-id={plan.id} className="ct-canvas-node" style={{ ...frameStyle(variant), ...style }}>
      {isPlan && <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />}

      <div style={{ height: S.planHeaderHeight - (isPlan ? 3 : 0), boxSizing: "border-box", padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 6, flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {!isPlan && <FileText size={13} style={{ flexShrink: 0, opacity: 0.7 }} />}
          <span title={plan.title} className="ct-canvas-title" style={{ fontWeight: 700, flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", letterSpacing: "-0.01em" }}>
            {isPlan ? plan.title : "Specification"}
          </span>
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
        {isPlan && (
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.86em" }}>
            {owner && (
              <span data-testid="canvas-plan-owner" style={{ display: "flex", alignItems: "center", gap: 5, maxWidth: 130, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", padding: "1px 8px 1px 6px", borderRadius: 999, background: "var(--ct-bg-selected)", border: "1px solid var(--ct-border-subtle)", opacity: "var(--ct-text-secondary-opacity)" }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: "rgba(56,189,248,0.95)", flexShrink: 0 }} />
                {owner}
              </span>
            )}
            <CoverageBar pct={pct} />
            <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>{pct}%</span>
          </div>
        )}
      </div>

      <div
        ref={bodyRef}
        className="ct-canvas-scroll"
        data-testid={isPlan ? "canvas-plan-body" : "canvas-spec-body"}
        data-content={!isPlan && contentNear ? "body" : "index"}
        data-canvas-scroll-body={!isPlan && contentNear ? "1" : undefined}
        style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflowY: !isPlan && contentNear ? "auto" : "hidden" }}
      >
        {rows.length === 0 ? (
          <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.9em", opacity: "var(--ct-text-tertiary-opacity)" }}>
            {isPlan ? `${plan.tasks.length} standalone task${plan.tasks.length === 1 ? "" : "s"}` : "no spec sections"}
          </div>
        ) : (
          rows.map((row) => (
            <SectionRowView key={row.section.id} row={row} variant={variant} contentNear={contentNear} />
          ))
        )}
      </div>
    </div>
  );
}

// One section: PLAN renders a compact index row (glyph·title·[n]·output dot); SPEC
// renders a block — title always, prose revealed on zoom (content-LOD). Evenly
// flex-distributed so PLAN rows and SPEC blocks line up for the 1:1 edges.
function SectionRowView({ row, variant, contentNear }: { row: SectionRow; variant: "plan" | "spec"; contentNear: boolean }) {
  const g = GLYPH[row.glyph];
  const isPlan = variant === "plan";
  const hasBody = row.section.body.trim().length > 0;
  const showProse = !isPlan && contentNear && hasBody;

  return (
    <div
      data-testid={isPlan ? "canvas-plan-row" : "canvas-spec-block"}
      data-section-id={row.section.id}
      data-row-glyph={row.glyph}
      style={{
        flex: showProse ? "0 0 auto" : 1,
        minHeight: 0,
        position: "relative",
        display: "flex",
        flexDirection: "column",
        gap: 4,
        padding: showProse ? "8px 12px" : "0 4px 0 8px",
        justifyContent: "center",
        borderLeft: `2px solid ${g.rail}`,
        borderBottom: showProse ? "1px solid var(--ct-border-subtle)" : "none",
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: row.depth * 8 }}>
        <g.Icon size={12} color={g.color} style={{ flexShrink: 0 }} strokeWidth={row.glyph === "uncovered" ? 2.4 : 2.2} />
        <span className="ct-canvas-title" style={{ flex: 1, minWidth: 0, fontSize: "0.84em", fontWeight: row.glyph === "uncovered" ? 600 : 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {row.section.title}
        </span>
        {isPlan && <span data-testid="canvas-plan-row-count" style={{ flexShrink: 0, fontSize: "0.68em", fontFamily: "var(--ct-font-mono)", fontWeight: 600, opacity: "var(--ct-text-secondary-opacity)", paddingRight: 8 }}>{row.taskCount}</span>}
      </div>
      {showProse && (
        <div data-testid="canvas-spec-section-body" style={{ fontSize: "0.82em", lineHeight: 1.5, opacity: 0.9, overflowWrap: "anywhere", paddingLeft: row.depth * 8 }}>
          <RichMarkdown text={row.section.body} />
        </div>
      )}
      {/* Per-section anchor dot on the node's right edge: PLAN output → tasks; SPEC → plan row. */}
      <span data-testid={isPlan ? "canvas-plan-output" : "canvas-spec-output"} aria-hidden style={{ position: "absolute", right: -1, top: showProse ? 14 : "50%", transform: showProse ? "none" : "translateY(-50%)", width: 7, height: 7, borderRadius: "50%", background: g.rail, border: "1px solid var(--ct-bg)" }} />
    </div>
  );
}

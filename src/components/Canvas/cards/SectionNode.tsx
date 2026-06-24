import { useRef, type CSSProperties } from "react";
import { Archive, Check, CircleDot, FileText, Maximize2, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan } from "../types";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { atLeast, type World } from "../worlds";
import { sectionRows, type SectionRow, type SectionRowGlyph } from "../sectionRows";
import { ownerName } from "../owner";
import { planCoverageFraction } from "./coverage";
import { digestMarkdown } from "./digest";
import type { CanvasCardStyleProps } from "./types";

const S = DEFAULT_CANVAS_SETTINGS;

// §24.3 / §26: SPEC and PLAN are two connected sectioned nodes (variant), each a
// FIXED frame whose CONTENT adapts to the node's appW WORLD band (§26.2):
//   W0 mass     — header + coverage + "[N sections]" (territory read, no body).
//   W1 structure— all section rows (title · ready · [n] · output dot).
//   W2 content  — bodies reveal (the bug fix): SPEC prose markdown, PLAN excerpts.
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
  onArchive?: () => void;
  onFitNode?: () => void;
}

export function SectionNode({ variant, plan, style, world, onArchive, onFitNode }: SectionNodeProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const owner = ownerName(plan.owner);
  const isPlan = variant === "plan";
  const testid = isPlan ? "canvas-plan-node" : "canvas-spec-node";
  const showRows = atLeast(world, "W1");
  const showBody = atLeast(world, "W2"); // the reveal band (§26.2)
  // W2+ bodies can overflow → node-internal scroll inside the fixed frame (§23.6).
  const scroll = showBody;

  return (
    <div data-testid={testid} data-plan-id={plan.id} data-world={world} className="ct-canvas-node" style={{ ...frameStyle(variant), ...style }}>
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
        {isPlan ? (
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
        ) : (
          <span style={{ fontSize: "0.76em", opacity: "var(--ct-text-tertiary-opacity)" }}>{rows.length} section{rows.length === 1 ? "" : "s"}</span>
        )}
      </div>

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
          rows.map((row) => <SectionRowView key={row.section.id} row={row} variant={variant} world={world} />)
        ) : (
          <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.84em", opacity: "var(--ct-text-tertiary-opacity)" }}>
            {rows.length} section{rows.length === 1 ? "" : "s"}
          </div>
        )}
      </div>
    </div>
  );
}

// One section. PLAN row = index (glyph·title·[n]·output dot) + at W2 a clamped
// excerpt, W3 fuller. SPEC block = title + 1-line abstract at W1, full prose at W2+.
function SectionRowView({ row, variant, world }: { row: SectionRow; variant: "plan" | "spec"; world: World }) {
  const g = GLYPH[row.glyph];
  const isPlan = variant === "plan";
  const hasBody = row.section.body.trim().length > 0;
  const showBody = atLeast(world, "W2") && hasBody;
  const showAbstract = !isPlan && !showBody && atLeast(world, "W1") && hasBody;
  const clamp = isPlan || !atLeast(world, "W3"); // SPEC unclamps at W3; PLAN stays excerpt
  const grow = showBody || showAbstract;

  return (
    <div
      data-testid={isPlan ? "canvas-plan-row" : "canvas-spec-block"}
      data-section-id={row.section.id}
      data-row-glyph={row.glyph}
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
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: row.depth * 8 }}>
        <g.Icon size={12} color={g.color} style={{ flexShrink: 0 }} strokeWidth={row.glyph === "uncovered" ? 2.4 : 2.2} />
        <span className="ct-canvas-title" style={{ flex: 1, minWidth: 0, fontSize: "0.84em", fontWeight: row.glyph === "uncovered" ? 600 : 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {row.section.title}
        </span>
        {isPlan && <span data-testid="canvas-plan-row-count" style={{ flexShrink: 0, fontSize: "0.68em", fontFamily: "var(--ct-font-mono)", fontWeight: 600, opacity: "var(--ct-text-secondary-opacity)", paddingRight: 8 }}>{row.taskCount}</span>}
      </div>
      {showAbstract && (
        <div data-testid="canvas-spec-abstract" style={{ fontSize: "0.78em", lineHeight: 1.4, opacity: "var(--ct-text-secondary-opacity)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", paddingLeft: row.depth * 8 }}>
          {digestMarkdown(row.section.body, 80)}
        </div>
      )}
      {showBody && (
        <div
          data-testid={isPlan ? "canvas-plan-section-body" : "canvas-spec-section-body"}
          className="ct-canvas-fade-in"
          style={clamp
            ? { fontSize: "0.82em", lineHeight: 1.5, opacity: 0.9, overflowWrap: "anywhere", paddingLeft: row.depth * 8, display: "-webkit-box", WebkitLineClamp: isPlan ? 4 : 8, WebkitBoxOrient: "vertical", overflow: "hidden" }
            : { fontSize: "0.82em", lineHeight: 1.55, opacity: 0.92, overflowWrap: "anywhere", paddingLeft: row.depth * 8 }}
        >
          <RichMarkdown text={row.section.body} />
        </div>
      )}
      {/* Per-section anchor dot on the node's right edge: PLAN output → tasks; SPEC → plan row. */}
      <span data-testid={isPlan ? "canvas-plan-output" : "canvas-spec-output"} aria-hidden style={{ position: "absolute", right: -1, top: grow ? 14 : "50%", transform: grow ? "none" : "translateY(-50%)", width: 7, height: 7, borderRadius: "50%", background: g.rail, border: "1px solid var(--ct-bg)" }} />
    </div>
  );
}

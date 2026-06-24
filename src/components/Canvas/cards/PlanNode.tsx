import type { CSSProperties } from "react";
import { Archive, Check, CircleDot, TriangleAlert } from "lucide-react";
import type { Plan } from "../types";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { sectionRows, type SectionRow, type SectionRowGlyph } from "../sectionRows";
import { ownerName } from "../owner";
import { planCoverageFraction } from "./coverage";
import type { CanvasCardStyleProps } from "./types";

const S = DEFAULT_CANVAS_SETTINGS;

// The hero of the board: elevated surface + prominent shadow + accent rail so
// the eye lands here first, with the spec reading as a recessed reference doc.
const frameStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-lg)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "0 10px 28px -8px rgba(0, 0, 0, 0.45), 0 2px 6px -2px rgba(0, 0, 0, 0.3)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
};

// LOCKED values (stop oscillating): covered rows = 0.18 fill + 3px accent rail
// (the primary coverage signal); uncovered = stronger amber + dashed so it wins.
const GLYPH = {
  done: { Icon: Check, color: "rgba(52, 211, 153, 1)", tint: "rgba(52, 211, 153, 0.18)", rail: "rgba(52, 211, 153, 0.9)", dashed: false },
  progress: { Icon: CircleDot, color: "rgba(129, 140, 248, 1)", tint: "rgba(99, 102, 241, 0.18)", rail: "rgba(99, 102, 241, 0.95)", dashed: false },
  uncovered: { Icon: TriangleAlert, color: "rgba(251, 191, 36, 1)", tint: "rgba(251, 191, 36, 0.26)", rail: "rgba(251, 191, 36, 1)", dashed: true },
} as const satisfies Record<SectionRowGlyph, { Icon: typeof Check; color: string; tint: string; rail: string; dashed: boolean }>;

function Row({ row }: { row: SectionRow }) {
  const { Icon, color, tint, rail, dashed } = GLYPH[row.glyph];
  const uncovered = row.glyph === "uncovered";
  return (
    <div
      data-testid="canvas-plan-row"
      data-section-id={row.section.id}
      data-row-glyph={row.glyph}
      style={{
        height: S.planRowHeight,
        boxSizing: "border-box",
        display: "flex",
        alignItems: "center",
        gap: 8,
        paddingRight: 12,
        paddingLeft: 10 + row.depth * S.planRowIndent,
        borderLeft: `3px ${dashed ? "dashed" : "solid"} ${rail}`,
        background: tint,
      }}
    >
      <Icon size={13} color={color} style={{ flexShrink: 0 }} strokeWidth={uncovered ? 2.4 : 2.2} />
      <span
        className="ct-canvas-title"
        style={{
          flex: 1,
          minWidth: 0,
          fontWeight: uncovered ? 600 : 500,
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
          opacity: uncovered ? 1 : 0.92,
        }}
      >
        {row.section.title}
      </span>
      <span
        data-testid="canvas-plan-row-count"
        style={{
          flexShrink: 0,
          minWidth: 20,
          textAlign: "center",
          fontSize: "0.74em",
          fontFamily: "var(--ct-font-mono)",
          fontWeight: 600,
          padding: "1px 6px",
          borderRadius: 999,
          background: uncovered ? "rgba(251, 191, 36, 0.18)" : "var(--ct-bg-active)",
          color: uncovered ? "rgba(251, 191, 36, 0.98)" : "var(--ct-fg)",
        }}
      >
        {row.taskCount}
      </span>
    </div>
  );
}

function CoverageBar({ pct }: { pct: number }) {
  return (
    <div
      title={`${pct}% of sections covered`}
      style={{ flex: 1, height: 6, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}
    >
      <div
        data-testid="canvas-plan-coverage-bar"
        style={{
          width: `${pct}%`,
          height: "100%",
          background: pct >= 100 ? "rgba(52, 211, 153, 0.95)" : "var(--ct-border-focus)",
        }}
      />
    </div>
  );
}

export interface PlanNodeProps extends CanvasCardStyleProps {
  plan: Plan;
  onArchive?: () => void;
}

export function PlanNode({ plan, style, viewMode = "document", onArchive }: PlanNodeProps) {
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const taskCount = plan.tasks.length;
  const doneCount = plan.tasks.filter((t) => t.status === "done").length;
  const standalone = rows.length === 0;
  const owner = ownerName(plan.owner);
  const accent = <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />;
  const coverGlyph = pct >= 100 ? "rgba(52,211,153,0.95)" : pct > 0 ? "rgba(129,140,248,1)" : "rgba(251,191,36,1)";

  const frame = (children: React.ReactNode) => (
    <div data-testid="canvas-plan-node" data-plan-id={plan.id} data-view-mode={viewMode} className="ct-canvas-node" style={{ ...frameStyle, ...style }}>
      {children}
    </div>
  );

  if (viewMode === "tile") {
    return frame(
      <>
        {accent}
        <div style={{ flex: 1, display: "flex", alignItems: "center", gap: 9, padding: "0 14px", minWidth: 0 }}>
          <span style={{ width: 9, height: 9, borderRadius: "50%", background: coverGlyph, flexShrink: 0 }} />
          <span
            title={plan.title}
            className="ct-canvas-title"
            style={{ flex: 1, minWidth: 0, fontWeight: 700, fontSize: "1.08em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
          >
            {plan.title}
          </span>
          <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", fontSize: "0.82em", opacity: "var(--ct-text-secondary-opacity)", flexShrink: 0 }}>
            {pct}%
          </span>
          <span data-testid="canvas-plan-done" style={{ fontVariantNumeric: "tabular-nums", fontFamily: "var(--ct-font-mono)", fontSize: "0.78em", flexShrink: 0 }}>
            {doneCount}/{taskCount}
          </span>
        </div>
      </>,
    );
  }

  const Header = (
    <div style={{ height: S.planHeaderHeight, boxSizing: "border-box", padding: "9px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 5, flexShrink: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span
          title={plan.title}
          className="ct-canvas-title"
          style={{
            fontWeight: 700,
            flex: 1,
            minWidth: 0,
            display: "-webkit-box",
            WebkitLineClamp: 2,
            WebkitBoxOrient: "vertical",
            overflow: "hidden",
            overflowWrap: "anywhere",
            lineHeight: 1.2,
            letterSpacing: "-0.01em",
          }}
        >
          {plan.title}
        </span>
        {onArchive && (
          <button
            type="button"
            data-testid="canvas-plan-archive"
            title="Archive plan"
            onClick={(e) => {
              e.stopPropagation();
              onArchive();
            }}
            onMouseDown={(e) => e.stopPropagation()}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: 24,
              height: 24,
              borderRadius: "var(--ct-radius-sm)",
              border: "1px solid var(--ct-border-subtle)",
              background: "var(--ct-bg)",
              color: "var(--ct-fg)",
              cursor: "pointer",
              flexShrink: 0,
            }}
          >
            <Archive size={13} />
          </button>
        )}
      </div>
      {owner && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.92em" }}>
          <span
            data-testid="canvas-plan-owner"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 5,
              maxWidth: 160,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              padding: "1px 8px 1px 6px",
              borderRadius: 999,
              background: "var(--ct-bg-selected)",
              border: "1px solid var(--ct-border-subtle)",
              opacity: "var(--ct-text-secondary-opacity)",
            }}
          >
            <span style={{ width: 6, height: 6, borderRadius: "50%", background: "rgba(56,189,248,0.95)", flexShrink: 0 }} />
            {owner}
          </span>
        </div>
      )}
    </div>
  );

  const Footer = !standalone && (
    <div
      style={{
        height: S.planFooterHeight,
        boxSizing: "border-box",
        padding: "0 12px",
        borderTop: "1px solid var(--ct-border-subtle)",
        display: "flex",
        alignItems: "center",
        gap: 8,
        fontSize: "0.92em",
      }}
    >
      <span data-testid="canvas-plan-done" style={{ fontVariantNumeric: "tabular-nums", opacity: "var(--ct-text-secondary-opacity)", flexShrink: 0 }}>
        {doneCount}/{taskCount} tasks
      </span>
      <CoverageBar pct={pct} />
      <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>
        {pct}% covered
      </span>
    </div>
  );

  if (viewMode === "card") {
    return frame(
      <>
        {accent}
        {Header}
        <div style={{ flex: 1, display: "flex", alignItems: "center", gap: 10, padding: "0 12px", fontSize: "0.92em", minWidth: 0 }}>
          <span style={{ display: "flex", alignItems: "center", gap: 6, opacity: "var(--ct-text-secondary-opacity)", flexShrink: 0 }}>
            {standalone ? `${taskCount} task${taskCount === 1 ? "" : "s"}` : `${rows.length} section${rows.length === 1 ? "" : "s"}`}
          </span>
          {!standalone && <CoverageBar pct={pct} />}
          <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>
            {pct}%
          </span>
        </div>
        {Footer}
      </>,
    );
  }

  return frame(
    <>
      {accent}
      {Header}
      <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}>
        {standalone ? (
          <div
            style={{
              flex: 1,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: "0.92em",
              opacity: "var(--ct-text-tertiary-opacity)",
            }}
          >
            {taskCount} standalone task{taskCount === 1 ? "" : "s"}
          </div>
        ) : (
          rows.map((row) => <Row key={row.section.id} row={row} />)
        )}
      </div>
      {Footer}
    </>,
  );
}

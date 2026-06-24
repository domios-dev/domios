import { useRef, type CSSProperties } from "react";
import { Archive, Check, CircleDot, FileText, LayoutGrid, Maximize2, Square, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan } from "../types";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { sectionRows, type SectionRow, type SectionRowGlyph } from "../sectionRows";
import { ownerName } from "../owner";
import { planCoverageFraction } from "./coverage";
import { digestMarkdown } from "./digest";
import type { CanvasCardStyleProps, CanvasViewMode } from "./types";

const S = DEFAULT_CANVAS_SETTINGS;

// ONE SectionedPlanNode (§22.2/§23.1): header + always-rendered right-edge rail
// (TOC + task-output anchors) + a FIXED-frame body per MANUAL mode. Merges the old
// plan-node + spec-node; spec prose = the section bodies. Zoom is camera-only.
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

const GLYPH = {
  done: { Icon: Check, color: "rgba(52, 211, 153, 1)", rail: "rgba(52, 211, 153, 0.9)" },
  progress: { Icon: CircleDot, color: "rgba(129, 140, 248, 1)", rail: "rgba(99, 102, 241, 0.95)" },
  uncovered: { Icon: TriangleAlert, color: "rgba(251, 191, 36, 1)", rail: "rgba(251, 191, 36, 1)" },
} as const satisfies Record<SectionRowGlyph, { Icon: typeof Check; color: string; rail: string }>;

const MODE_BTN: { mode: CanvasViewMode; Icon: typeof Square; title: string }[] = [
  { mode: "document", Icon: FileText, title: "Document" },
  { mode: "card", Icon: LayoutGrid, title: "Card" },
  { mode: "tile", Icon: Square, title: "Tile" },
];

function CoverageBar({ pct }: { pct: number }) {
  return (
    <div title={`${pct}% covered`} style={{ flex: 1, height: 6, borderRadius: 3, background: "var(--ct-bg-active)", overflow: "hidden" }}>
      <div
        data-testid="canvas-plan-coverage-bar"
        style={{ width: `${pct}%`, height: "100%", background: pct >= 100 ? "rgba(52,211,153,0.95)" : "var(--ct-border-focus)" }}
      />
    </div>
  );
}

export interface SectionedPlanNodeProps extends CanvasCardStyleProps {
  plan: Plan;
  contentNear?: boolean;
  onArchive?: () => void;
  onSetMode?: (mode: CanvasViewMode) => void;
  onFitNode?: () => void;
}

export function SectionedPlanNode({ plan, style, viewMode = "document", contentNear = false, onArchive, onSetMode, onFitNode }: SectionedPlanNodeProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const rows = sectionRows(plan);
  const pct = Math.round(planCoverageFraction(plan) * 100);
  const taskCount = plan.tasks.length;
  const doneCount = plan.tasks.filter((t) => t.status === "done").length;
  const owner = ownerName(plan.owner);
  const standalone = rows.length === 0;

  const scrollToSection = (id: string) => {
    bodyRef.current?.querySelector(`[data-section-block="${id}"]`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  // Content-LOD (§23.7): the MODE sets the frame SIZE; camera zoom adapts the
  // CONTENT inside it. Near (zoomed in) reveals the section BODIES in ANY frame;
  // far falls back to the mode's baseline (tile=index, card=digests, doc=bodies).
  const showBody = viewMode === "document" || contentNear;
  const showDigest = viewMode === "card" && !contentNear;
  const showIndex = viewMode === "tile" && !contentNear;

  return (
    <div data-testid="canvas-plan-node" data-plan-id={plan.id} data-view-mode={viewMode} className="ct-canvas-node" style={{ ...frameStyle, ...style }}>
      <div style={{ height: 3, flexShrink: 0, background: "linear-gradient(90deg, var(--ct-border-focus), rgba(52,211,153,0.85))" }} />

      <div style={{ height: S.planHeaderHeight - 3, boxSizing: "border-box", padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 6, flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span title={plan.title} className="ct-canvas-title" style={{ fontWeight: 700, flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", letterSpacing: "-0.01em" }}>
            {plan.title}
          </span>
          <div data-testid="canvas-plan-mode-switch" style={{ display: "flex", gap: 2, flexShrink: 0 }} onMouseDown={(e) => e.stopPropagation()}>
            {MODE_BTN.map(({ mode, Icon, title }) => (
              <button
                key={mode}
                type="button"
                data-testid={`canvas-plan-mode-${mode}`}
                data-active={viewMode === mode ? "true" : "false"}
                title={title}
                onClick={(e) => { e.stopPropagation(); onSetMode?.(mode); }}
                style={iconBtn(viewMode === mode)}
              >
                <Icon size={12} />
              </button>
            ))}
          </div>
          {onFitNode && (
            <button type="button" data-testid="canvas-plan-fit" title="Focus this plan" onClick={(e) => { e.stopPropagation(); onFitNode(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn(false)}>
              <Maximize2 size={12} />
            </button>
          )}
          {onArchive && (
            <button type="button" data-testid="canvas-plan-archive" title="Archive plan" onClick={(e) => { e.stopPropagation(); onArchive(); }} onMouseDown={(e) => e.stopPropagation()} style={iconBtn(false)}>
              <Archive size={12} />
            </button>
          )}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.86em" }}>
          {owner && (
            <span data-testid="canvas-plan-owner" style={{ display: "flex", alignItems: "center", gap: 5, maxWidth: 150, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", padding: "1px 8px 1px 6px", borderRadius: 999, background: "var(--ct-bg-selected)", border: "1px solid var(--ct-border-subtle)", opacity: "var(--ct-text-secondary-opacity)" }}>
              <span style={{ width: 6, height: 6, borderRadius: "50%", background: "rgba(56,189,248,0.95)", flexShrink: 0 }} />
              {owner}
            </span>
          )}
          <span data-testid="canvas-plan-done" style={{ fontVariantNumeric: "tabular-nums", opacity: "var(--ct-text-secondary-opacity)", flexShrink: 0 }}>{doneCount}/{taskCount}</span>
          <CoverageBar pct={pct} />
          <span data-testid="canvas-plan-coverage-pct" style={{ fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>{pct}%</span>
        </div>
      </div>

      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        <div
          ref={bodyRef}
          className="ct-canvas-scroll"
          data-testid="canvas-plan-body"
          data-content={showBody ? "body" : showDigest ? "digest" : "index"}
          data-canvas-scroll-body={showBody ? "1" : undefined}
          style={{ flex: 1, minWidth: 0, overflowY: showBody ? "auto" : "hidden", padding: showIndex ? 0 : "8px 12px" }}
        >
          {showIndex ? <TileBody rows={rows} taskCount={taskCount} standalone={standalone} /> : null}
          {showDigest ? <CardBody rows={rows} standalone={standalone} taskCount={taskCount} /> : null}
          {showBody ? (
            <div className="ct-canvas-fade-in" style={{ height: "100%" }}>
              <DocumentBody rows={rows} standalone={standalone} taskCount={taskCount} />
            </div>
          ) : null}
        </div>
        <Rail rows={rows} compact={viewMode === "tile" && !contentNear} onJump={scrollToSection} />
      </div>
    </div>
  );
}

function iconBtn(active: boolean): CSSProperties {
  return {
    display: "flex", alignItems: "center", justifyContent: "center", width: 22, height: 22,
    borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)",
    background: active ? "var(--ct-border-focus)" : "var(--ct-bg)", color: active ? "var(--ct-bg)" : "var(--ct-fg)",
    cursor: "pointer", flexShrink: 0,
  };
}

// Always-rendered right-edge rail: section rows hold the OUTPUT dots (вых) that
// tasks anchor to (evenly distributed → stable Y, matches railAnchorOffsets). In
// Tile the rail collapses to a thin dots-only stub.
function Rail({ rows, compact, onJump }: { rows: SectionRow[]; compact: boolean; onJump: (id: string) => void }) {
  return (
    <div
      data-testid="canvas-plan-rail"
      style={{ width: compact ? 18 : S.railW, flexShrink: 0, borderLeft: "1px solid var(--ct-border-subtle)", background: "color-mix(in srgb, var(--ct-fg) 3%, transparent)", display: "flex", flexDirection: "column" }}
    >
      {rows.map((row) => {
        const g = GLYPH[row.glyph];
        return (
          <div
            key={row.section.id}
            data-testid="canvas-plan-rail-row"
            data-section-id={row.section.id}
            data-row-glyph={row.glyph}
            title={row.section.title}
            onClick={(e) => { e.stopPropagation(); onJump(row.section.id); }}
            onMouseDown={(e) => e.stopPropagation()}
            style={{ flex: 1, minHeight: 0, display: "flex", alignItems: "center", gap: 5, padding: compact ? 0 : "0 4px 0 6px", cursor: "pointer", position: "relative", borderLeft: `2px solid ${g.rail}` }}
          >
            {!compact && (
              <>
                <g.Icon size={11} color={g.color} style={{ flexShrink: 0 }} strokeWidth={row.glyph === "uncovered" ? 2.4 : 2.2} />
                <span className="ct-canvas-title" style={{ flex: 1, minWidth: 0, fontSize: "0.78em", fontWeight: row.glyph === "uncovered" ? 600 : 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{row.section.title}</span>
                <span data-testid="canvas-plan-rail-count" style={{ flexShrink: 0, fontSize: "0.64em", fontFamily: "var(--ct-font-mono)", fontWeight: 600, opacity: "var(--ct-text-secondary-opacity)" }}>{row.taskCount}</span>
              </>
            )}
            {/* OUTPUT dot (вых) on the node's right edge — the task connection anchor. */}
            <span data-testid="canvas-plan-output" aria-hidden style={{ position: "absolute", right: -1, width: 7, height: 7, borderRadius: "50%", background: g.rail, border: "1px solid var(--ct-bg)" }} />
          </div>
        );
      })}
    </div>
  );
}

// Tile body = a compact SECTION INDEX (glyph · title · [n]), rows evenly
// distributed so they line up with the rail's output dots. Coverage lives in the
// header bar — no oversized panel. Reads as an index, not one green block.
function TileBody({ rows, taskCount, standalone }: { rows: SectionRow[]; taskCount: number; standalone: boolean }) {
  if (standalone) return <Standalone taskCount={taskCount} />;
  return (
    <div data-testid="canvas-plan-tile-index" style={{ height: "100%", display: "flex", flexDirection: "column" }}>
      {rows.map((row) => {
        const g = GLYPH[row.glyph];
        return (
          <div
            key={row.section.id}
            data-testid="canvas-plan-tile-row"
            data-section-id={row.section.id}
            style={{ flex: 1, minHeight: 0, display: "flex", alignItems: "center", gap: 6, padding: "0 10px", borderLeft: `2px solid ${g.rail}`, overflow: "hidden" }}
          >
            <g.Icon size={11} color={g.color} style={{ flexShrink: 0 }} strokeWidth={row.glyph === "uncovered" ? 2.4 : 2.2} />
            <span style={{ flex: 1, minWidth: 0, fontSize: "0.84em", fontWeight: row.glyph === "uncovered" ? 600 : 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{row.section.title}</span>
            <span style={{ flexShrink: 0, fontSize: "0.7em", fontFamily: "var(--ct-font-mono)", fontWeight: 600, opacity: "var(--ct-text-secondary-opacity)" }}>{row.taskCount}</span>
          </div>
        );
      })}
    </div>
  );
}

function CardBody({ rows, standalone, taskCount }: { rows: SectionRow[]; standalone: boolean; taskCount: number }) {
  if (standalone) return <Standalone taskCount={taskCount} />;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
      {rows.map((row) => {
        const g = GLYPH[row.glyph];
        const digest = digestMarkdown(row.section.body, 90);
        return (
          <div key={row.section.id} data-testid="canvas-plan-card-row" style={{ display: "flex", gap: 7, alignItems: "flex-start" }}>
            <g.Icon size={12} color={g.color} style={{ flexShrink: 0, marginTop: "0.2em" }} strokeWidth={2.2} />
            <div style={{ minWidth: 0, flex: 1 }}>
              <div style={{ fontWeight: 600, fontSize: "0.92em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{row.section.title}</div>
              {digest && <div style={{ fontSize: "0.84em", opacity: "var(--ct-text-secondary-opacity)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{digest}</div>}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function DocumentBody({ rows, standalone, taskCount }: { rows: SectionRow[]; standalone: boolean; taskCount: number }) {
  if (standalone) return <Standalone taskCount={taskCount} />;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {rows.map((row) => {
        const g = GLYPH[row.glyph];
        const hasBody = row.section.body.trim().length > 0;
        return (
          <div key={row.section.id} data-section-block={row.section.id} data-testid="canvas-plan-doc-section" style={{ scrollMarginTop: 8 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4, paddingLeft: row.depth * 10 }}>
              <g.Icon size={12} color={g.color} style={{ flexShrink: 0 }} strokeWidth={2.2} />
              <span style={{ fontWeight: 700, fontSize: "0.96em", letterSpacing: "-0.005em" }}>{row.section.title}</span>
            </div>
            {hasBody && (
              <div data-testid="canvas-plan-doc-body" style={{ fontSize: "0.88em", lineHeight: 1.5, opacity: 0.92, overflowWrap: "anywhere", paddingLeft: row.depth * 10 }}>
                <RichMarkdown text={row.section.body} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function Standalone({ taskCount }: { taskCount: number }) {
  return (
    <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.92em", opacity: "var(--ct-text-tertiary-opacity)" }}>
      {taskCount} standalone task{taskCount === 1 ? "" : "s"}
    </div>
  );
}

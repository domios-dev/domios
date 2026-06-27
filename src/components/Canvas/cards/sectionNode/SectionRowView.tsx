import { Copy, Crosshair } from "lucide-react";
import { atLeast, type World } from "../../worlds";
import { type SectionRow } from "../../sectionRows";
import type { SectionNavArgs } from "../../nodeRegistry";
import { digestMarkdown } from "../digest";
import { GLYPH, SectionMarkdownBody, copyText, smallBtn } from "./shared";
import { SpecRefBubbles, type SpecRef } from "./SpecRefBubbles";

// One section. SPEC and PLAN both read as a DOCUMENT at W2+: title heading + full
// (unclamped) markdown body + facet tags, in a node-internal scroll column — §22.1
// reserve-at-deepest means W2→W3 changes only rasterization, never geometry. A PLAN
// section adds spec-reference bubbles naming the spec it derives from. SPEC keeps a
// 1-line abstract at W1. §32.7: single-click highlights connected tasks; double-
// click / target frames them; copy-id yanks the id. All camera motion is user-led.
export function SectionRowView({ row, variant, world, planId, specRefs, onSectionClick, onSectionFrame, suppressRichContent = false, stretchEmpty = true }: {
  row: SectionRow; variant: "plan" | "spec"; world: World; planId: string;
  specRefs?: SpecRef[];
  onSectionClick?: (a: SectionNavArgs) => void; onSectionFrame?: (a: SectionNavArgs) => void;
  suppressRichContent?: boolean; stretchEmpty?: boolean;
}) {
  const g = GLYPH[row.glyph];
  const isPlan = variant === "plan";
  const hasBody = row.section.body.trim().length > 0;
  const showBody = atLeast(world, "W2") && hasBody && !suppressRichContent;
  const showAbstract = !isPlan && !showBody && atLeast(world, "W1") && hasBody && !suppressRichContent;
  const deep = atLeast(world, "W3");
  // §section-facets: a section's own tags surface as neutral chips at W2+ (the
  // content band, where the row has room) — the same chip vocab task topics use, so
  // a facet reads identically on a plan section and on the tasks under it.
  const tags = (row.section.tags ?? []).filter((t) => t.trim());
  const showTags = atLeast(world, "W2") && tags.length > 0 && !suppressRichContent;
  const refs = (specRefs ?? []).filter((r) => r.id?.trim());
  const showSpecRefs = isPlan && atLeast(world, "W2") && refs.length > 0 && !suppressRichContent;
  // A revealed body turns the section into a document block: the title becomes a
  // heading (wraps, never mid-word clipped) so it reads top-to-bottom.
  const heading = showBody;
  const wrapTitle = heading || deep;
  const grow = showBody || showAbstract || showTags || showSpecRefs;
  const nav: SectionNavArgs = { planId, sectionId: row.section.id, variant };
  const showActions = atLeast(world, "W2");

  return (
    <div
      data-testid={isPlan ? "canvas-plan-row" : "canvas-spec-block"}
      data-section-id={row.section.id}
      data-row-glyph={row.glyph}
      // A click on an inline fly-to chip (spec bubble) must reach the delegated
      // canvas fly handler on the container — don't swallow it as a section select.
      onClick={onSectionClick ? (e) => { if ((e.target as HTMLElement).closest("[data-fly-to-id]")) return; e.stopPropagation(); onSectionClick(nav); } : undefined}
      onDoubleClick={onSectionFrame ? (e) => { if ((e.target as HTMLElement).closest("[data-fly-to-id]")) return; e.stopPropagation(); onSectionFrame(nav); } : undefined}
      style={{
        flex: grow || !stretchEmpty ? "0 0 auto" : 1,
        minHeight: grow || !stretchEmpty ? 36 : 0,
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
        <span data-canvas-selectable className="ct-canvas-title ct-canvas-text" style={{ flex: 1, minWidth: 0, fontSize: heading ? "0.94em" : "0.84em", fontWeight: heading ? 700 : row.glyph === "uncovered" ? 600 : 500, letterSpacing: heading ? "-0.01em" : undefined, ...(wrapTitle ? { whiteSpace: "normal", wordBreak: "break-word" } : { whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }) }}>
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
      {showTags && (
        <div data-testid="canvas-section-tags" style={{ display: "flex", flexWrap: "wrap", gap: 5, paddingLeft: row.depth * 8 }}>
          {tags.map((t) => (
            <span
              key={t}
              data-testid="canvas-section-tag"
              style={{ fontSize: "0.72em", maxWidth: 150, padding: "0.06em 0.5em", borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg-active)", color: "var(--ct-fg)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
            >
              {t}
            </span>
          ))}
        </div>
      )}
      {showAbstract && (
        <div data-testid="canvas-spec-abstract" data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.78em", lineHeight: 1.4, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", paddingLeft: row.depth * 8 }}>
          {digestMarkdown(row.section.body, 80)}
        </div>
      )}
      {showBody && (
        // Full markdown document — no line clamp at any band. The node's own scroll
        // column (canvas-plan-body / canvas-spec-body) takes the overflow, so a long
        // section reads top-to-bottom with nothing clipped.
        <div style={{ fontSize: "0.82em", lineHeight: 1.55, paddingLeft: row.depth * 8 }}>
          <SectionMarkdownBody
            text={row.section.body}
            testid={isPlan ? "canvas-plan-section-body" : "canvas-spec-section-body"}
          />
        </div>
      )}
      {showSpecRefs && (
        <div style={{ paddingLeft: row.depth * 8 }}>
          <SpecRefBubbles refs={refs} />
        </div>
      )}
      {/* Per-section anchor dot on the node's right edge: PLAN output → tasks; SPEC → plan row. */}
      <span data-testid={isPlan ? "canvas-plan-output" : "canvas-spec-output"} aria-hidden style={{ position: "absolute", right: -1, top: grow ? 14 : "50%", transform: grow ? "none" : "translateY(-50%)", width: 7, height: 7, borderRadius: "50%", background: g.rail, border: "1px solid var(--ct-bg)" }} />
    </div>
  );
}

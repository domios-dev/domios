import { Copy, Crosshair } from "lucide-react";
import { atLeast, type World } from "../../worlds";
import { type SectionRow } from "../../sectionRows";
import type { SectionNavArgs } from "../../nodeRegistry";
import { digestMarkdown } from "../digest";
import { GLYPH, SectionMarkdownBody, copyText, smallBtn } from "./shared";

// One section. PLAN row = index (glyph·title·count·output dot) + a CLAMPED excerpt
// at W2+ (no W2→W3 toggle). SPEC block = title + 1-line abstract at W1, then the
// DEEPEST (unclamped) prose layout the moment it reveals at W2 — §22.1 reserve-at-
// deepest applied to content: W2→W3 changes only rasterization, never geometry.
// §32.7: single-click highlights connected tasks; double-click / target button
// frames them; copy-id yanks the section id. All camera motion is user-initiated.
export function SectionRowView({ row, variant, world, planId, onSectionClick, onSectionFrame, suppressRichContent = false, stretchEmpty = true }: {
  row: SectionRow; variant: "plan" | "spec"; world: World; planId: string;
  onSectionClick?: (a: SectionNavArgs) => void; onSectionFrame?: (a: SectionNavArgs) => void;
  suppressRichContent?: boolean; stretchEmpty?: boolean;
}) {
  const g = GLYPH[row.glyph];
  const isPlan = variant === "plan";
  const hasBody = row.section.body.trim().length > 0;
  const showBody = atLeast(world, "W2") && hasBody && !suppressRichContent;
  const showAbstract = !isPlan && !showBody && atLeast(world, "W1") && hasBody && !suppressRichContent;
  const deep = atLeast(world, "W3");
  const clamp = isPlan && !deep; // §22.1 spec unclamped from W2; PLAN excerpt clamps at W2 but opens FULLY at W3 (deep read)
  // §section-facets: a section's own tags surface as neutral chips at W2+ (the
  // content band, where the row has room) — the same chip vocab task topics use, so
  // a facet reads identically on a plan section and on the tasks under it.
  const tags = (row.section.tags ?? []).filter((t) => t.trim());
  const showTags = atLeast(world, "W2") && tags.length > 0 && !suppressRichContent;
  const grow = showBody || showAbstract || showTags;
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

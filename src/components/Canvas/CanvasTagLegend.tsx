import type { CSSProperties } from "react";
import { isTagFilterSelected, legendEntries, type TagFilterSelection } from "./tagModel";
import type { TagFacet } from "./types";

const shell: CSSProperties = {
  position: "absolute",
  right: 12,
  bottom: 140,
  zIndex: 20,
  maxWidth: 200,
  padding: "6px 8px",
  borderRadius: "var(--ct-radius-md)",
  border: "1px solid var(--ct-border-default)",
  background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)",
  boxShadow: "0 8px 24px -10px rgba(0,0,0,0.55)",
  backdropFilter: "blur(8px)",
  fontFamily: "var(--ct-font-ui)",
  fontSize: 10,
  color: "var(--ct-fg)",
};

export interface CanvasTagLegendProps {
  facets: TagFacet[];
  filter: TagFilterSelection;
  onToggleTag: (tag: string) => void;
}

export function CanvasTagLegend({ facets, filter, onToggleTag }: CanvasTagLegendProps) {
  const groups = legendEntries(facets);
  if (!groups.length) return null;

  return (
    <div data-testid="canvas-tag-legend" style={shell} onMouseDown={(e) => e.stopPropagation()}>
      {groups.map((group) => (
        <div key={group.key} data-testid="canvas-tag-legend-facet" data-facet-key={group.key} style={{ marginBottom: 6 }}>
          <div style={{ fontWeight: 700, fontSize: "0.95em", marginBottom: 3, opacity: 0.85, textTransform: "capitalize" }}>{group.label}</div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
            {group.values.map((v) => {
              const selected = isTagFilterSelected(filter, v.tag);
              return (
                <button
                  key={v.tag}
                  type="button"
                  data-testid="canvas-tag-legend-value"
                  data-tag={v.tag}
                  data-selected={selected ? "true" : "false"}
                  title={selected ? `Clear filter: ${v.tag}` : `Filter: ${v.tag}`}
                  onClick={() => onToggleTag(v.tag)}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 4,
                    padding: "1px 5px",
                    borderRadius: 999,
                    border: `1px solid color-mix(in srgb, ${v.color} ${selected ? 85 : 55}%, transparent)`,
                    background: selected
                      ? `color-mix(in srgb, ${v.color} 32%, transparent)`
                      : `color-mix(in srgb, ${v.color} 14%, transparent)`,
                    whiteSpace: "nowrap",
                    cursor: "pointer",
                    font: "inherit",
                    color: "inherit",
                    boxShadow: selected ? `0 0 0 1px color-mix(in srgb, ${v.color} 45%, transparent)` : undefined,
                  }}
                >
                  <span aria-hidden style={{ width: 6, height: 6, borderRadius: "50%", background: v.color, flexShrink: 0 }} />
                  {v.label}
                </button>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

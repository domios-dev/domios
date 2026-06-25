import type { CSSProperties } from "react";
import { legendEntries } from "./tagModel";
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
}

export function CanvasTagLegend({ facets }: CanvasTagLegendProps) {
  const groups = legendEntries(facets);
  if (!groups.length) return null;

  return (
    <div data-testid="canvas-tag-legend" style={shell} onMouseDown={(e) => e.stopPropagation()}>
      {groups.map((group) => (
        <div key={group.key} data-testid="canvas-tag-legend-facet" data-facet-key={group.key} style={{ marginBottom: 6 }}>
          <div style={{ fontWeight: 700, fontSize: "0.95em", marginBottom: 3, opacity: 0.85, textTransform: "capitalize" }}>{group.label}</div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
            {group.values.map((v) => (
              <span
                key={v.tag}
                data-testid="canvas-tag-legend-value"
                data-tag={v.tag}
                title={v.tag}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 4,
                  padding: "1px 5px",
                  borderRadius: 999,
                  border: `1px solid color-mix(in srgb, ${v.color} 55%, transparent)`,
                  background: `color-mix(in srgb, ${v.color} 14%, transparent)`,
                  whiteSpace: "nowrap",
                }}
              >
                <span aria-hidden style={{ width: 6, height: 6, borderRadius: "50%", background: v.color, flexShrink: 0 }} />
                {v.label}
              </span>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

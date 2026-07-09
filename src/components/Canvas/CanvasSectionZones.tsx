import { memo } from "react";
import type { SectionZone } from "./layout";
import { smoothClosedPath, zoneLabelBox, ZONE_LABEL_FONT } from "./layout/sectionZones";
import { atLeast, type World } from "./worlds";


const MAX_ZONES = 64;

interface CanvasSectionZonesProps {
  zones: readonly SectionZone[];
  world: World;
  part: "hull" | "label";
  /** sectionId → title from the plan's OWN sections (layout plan ids are spec-overlaid). */
  titleById?: ReadonlyMap<string, string>;
}

export const CanvasSectionZones = memo(function CanvasSectionZones({ zones, world, part, titleById }: CanvasSectionZonesProps) {
  if (!zones.length) return null;
  // Recede at deep read; the territory glance lives at W1/W2.
  const fade = atLeast(world, "W3") ? 0.45 : 1;
  const shown = zones.slice(0, MAX_ZONES);

  if (part === "hull") {
    return (
      <g data-testid="canvas-section-zones">
        {shown.map((z) => (
          <path
            key={z.sectionId}
            data-testid="canvas-section-zone"
            data-section-id={z.sectionId}
            d={smoothClosedPath(z.hull)}
            fill={z.color}
            fillOpacity={0.1 * fade}
            stroke={z.color}
            strokeOpacity={0.5 * fade}
            strokeWidth={2}
            strokeLinejoin="round"
          />
        ))}
      </g>
    );
  }

  return (
    <g data-testid="canvas-section-zone-labels">
      {shown.map((z) => {
        const text = titleById?.get(z.sectionId) ?? z.label;
        if (!text) return null;
        const box = zoneLabelBox(text);
        const lineH = ZONE_LABEL_FONT * 1.28;
        const y0 = -((box.lines.length - 1) * lineH) / 2;
        return (
          <g
            key={z.sectionId}
            data-testid="canvas-section-zone-label"
            data-section-id={z.sectionId}
            transform={`translate(${z.labelAnchor.x}, ${z.labelAnchor.y})`}
            opacity={fade}
          >
            <rect
              x={-box.width / 2}
              y={-box.height / 2}
              width={box.width}
              height={box.height}
              rx={Math.min(box.height, 22) / 2 + 6}
              fill="var(--ct-bg)"
              fillOpacity={0.92}
              stroke={z.color}
              strokeOpacity={0.7}
              strokeWidth={1.5}
            />
            <text
              textAnchor="middle"
              dominantBaseline="central"
              fontSize={ZONE_LABEL_FONT}
              fontWeight={800}
              fill={z.color}
              style={{ fontFamily: "var(--ct-font-ui)", letterSpacing: "0.01em" }}
            >
              {box.lines.map((line, i) => (
                <tspan key={i} x={0} y={y0 + i * lineH}>{line}</tspan>
              ))}
            </text>
          </g>
        );
      })}
    </g>
  );
});

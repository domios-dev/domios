import { Milestone, Tags } from "lucide-react";
import type { Plan } from "../../types";
import { MILESTONE_LANE_COLORS } from "../../settings";
import { atLeast, type World } from "../../worlds";
import { Eyebrow } from "../provenanceShell";
import { VOCAB_ACCENT, metaChip } from "./shared";

// §vocabulary: the plan's DECLARED vocabularies — milestone kinds (with their lane
// colors) and tag facets. Reference material, not glance-critical: revealed inside
// the scroll body at W2 (kinds) / W3 (facets), so the deep dive shows the full
// legend via node-internal scroll and the far glance stays uncluttered.
export function VocabularyLegend({ plan, world }: { plan: Plan; world: World }) {
  const kinds = plan.milestone_kinds ?? [];
  const facets = (plan.tag_facets ?? []).filter((f) => f.values.length > 0);
  const deep = atLeast(world, "W3");
  const maxKinds = deep ? 32 : 6;
  const shownKinds = kinds.slice(0, maxKinds);
  const showFacets = deep && facets.length > 0;

  return (
    <div data-testid="canvas-plan-vocab" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 8 }}>
      {kinds.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
          <Eyebrow icon={<Milestone size={12} strokeWidth={2.2} />} label="Milestone kinds" accent={VOCAB_ACCENT} testid="canvas-plan-kinds-eyebrow" />
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {shownKinds.map((k, i) => {
              const color = k.color ?? MILESTONE_LANE_COLORS[i % MILESTONE_LANE_COLORS.length];
              return (
                <span key={k.key} data-testid="canvas-plan-kind" title={k.label ?? k.key} style={metaChip(color)}>
                  <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />
                  <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{k.label ?? k.key}</span>
                </span>
              );
            })}
            {kinds.length > maxKinds && <span style={{ fontSize: "0.72em", opacity: 0.6, alignSelf: "center" }}>+{kinds.length - maxKinds} more</span>}
          </div>
        </div>
      )}
      {showFacets && facets.map((facet) => (
        <div key={facet.key} style={{ display: "flex", flexDirection: "column", gap: 5 }}>
          <Eyebrow icon={<Tags size={12} strokeWidth={2.2} />} label={facet.label ?? facet.key} accent={VOCAB_ACCENT} testid="canvas-plan-facet-eyebrow" />
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {facet.values.map((val) => {
              const color = val.color ?? VOCAB_ACCENT;
              return (
                <span key={val.v} data-testid="canvas-plan-facet-value" title={val.label ?? val.v} style={metaChip(color)}>
                  {val.color && <span aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />}
                  <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{val.label ?? val.v}</span>
                </span>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

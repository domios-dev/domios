import { CornerDownRight, FileText } from "lucide-react";
import { LINK_ACCENT, metaChip, shortId } from "./shared";

export interface SpecRef {
  id: string;
  title: string;
}

// §plan-as-doc: a plan section is the HOW; these bubbles name the spec section(s) —
// the WHAT — it was written from. Clicking flies to that spec section (the delegated
// data-fly-to-* handler). The bubbles REPLACE node-to-node edges: provenance reads
// inline on the section it belongs to, never as a wire across the board.
export function SpecRefBubbles({ refs }: { refs: SpecRef[] }) {
  if (!refs.length) return null;
  return (
    <div
      data-testid="canvas-plan-spec-refs"
      style={{ display: "flex", flexWrap: "wrap", gap: 5, alignItems: "center", marginTop: 2 }}
    >
      <span
        aria-hidden
        style={{
          display: "inline-flex", alignItems: "center", gap: 4,
          fontSize: "0.66em", fontWeight: 800, letterSpacing: "0.04em", textTransform: "uppercase",
          color: LINK_ACCENT, opacity: 0.85,
        }}
      >
        <CornerDownRight size={11} strokeWidth={2.4} /> From spec
      </span>
      {refs.map((r) => (
        <span
          key={r.id}
          data-testid="canvas-plan-spec-ref"
          data-fly-to-kind="spec-section"
          data-fly-to-id={r.id}
          role="button"
          tabIndex={0}
          title={`Fly to spec section: ${r.title || r.id}`}
          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.currentTarget.click(); } }}
          onMouseDown={(e) => e.stopPropagation()}
          style={metaChip(LINK_ACCENT, true)}
        >
          <FileText size={10} strokeWidth={2.2} aria-hidden style={{ flexShrink: 0, opacity: 0.85 }} />
          <span style={{ maxWidth: 210, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {r.title || shortId(r.id)}
          </span>
        </span>
      ))}
    </div>
  );
}

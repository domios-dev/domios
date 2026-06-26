import { ArrowUpRight, FileText, Link2, Replace } from "lucide-react";
import type { Plan } from "../../types";
import { atLeast, type World } from "../../worlds";
import { Eyebrow } from "../provenanceShell";
import { LINK_ACCENT, SUPERSEDE_ACCENT, metaChip, shortId } from "./shared";

// §linkage: the plan's edges to OTHER efforts — the spec(s) it binds to and its
// lineage (follows / supersedes). Far glance = a single spec-bound chip + lineage
// count; W2 adds the bound-section count; W3 reveals short ids. Each chip carries
// data-fly-to-* so a later iter can POST /api/canvas/focus to the linked node;
// today click yanks the id (immediately useful, fly-ready).
export function LinkageStrip({ plan, world }: { plan: Plan; world: World }) {
  const specId = plan.primary_spec_id?.trim() ?? "";
  const links = plan.spec_links ?? [];
  const lineage = (plan.lineage ?? []).filter((l) => l.to?.trim());
  const boundSections = new Set(links.map((l) => l.plan_section_id)).size;
  const resolvedSpec = specId || links[0]?.spec_id || "";
  const detail = atLeast(world, "W2");
  const deep = atLeast(world, "W3");

  return (
    <div data-testid="canvas-plan-linkage" className="ct-canvas-fade-in" style={{ flexShrink: 0, padding: "6px 12px", borderBottom: "1px solid var(--ct-border-subtle)", display: "flex", flexDirection: "column", gap: 5 }}>
      <Eyebrow icon={<Link2 size={12} strokeWidth={2.2} />} label="Linked" accent={LINK_ACCENT} testid="canvas-plan-linkage-eyebrow" />
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
        {resolvedSpec && (
          <span
            data-testid="canvas-plan-spec-link"
            data-fly-to-kind="spec"
            data-fly-to-id={resolvedSpec}
            role="button"
            tabIndex={0}
            title={`Fly to bound spec: ${resolvedSpec}${boundSections ? `\n${boundSections} section link${boundSections === 1 ? "" : "s"}` : ""}`}
            onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.currentTarget.click(); } }}
            onMouseDown={(e) => e.stopPropagation()}
            style={metaChip(LINK_ACCENT, true)}
          >
            <FileText size={11} strokeWidth={2.2} aria-hidden style={{ flexShrink: 0, opacity: 0.85 }} />
            <span style={{ fontWeight: 700 }}>Spec</span>
            {detail && boundSections > 0 && (
              <span style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.9em", opacity: 0.7 }}>{boundSections}§</span>
            )}
            {deep && <span style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.86em", opacity: 0.55 }}>{shortId(resolvedSpec)}</span>}
          </span>
        )}
        {lineage.map((link, i) => {
          const sup = link.rel === "supersedes";
          const accent = sup ? SUPERSEDE_ACCENT : LINK_ACCENT;
          return (
            <span
              key={`${link.rel}:${link.to}:${i}`}
              data-testid="canvas-plan-lineage"
              data-fly-to-kind={link.rel}
              data-fly-to-id={link.to}
              role="button"
              tabIndex={0}
              title={`Fly to ${link.rel}: ${link.to}`}
              onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.currentTarget.click(); } }}
              onMouseDown={(e) => e.stopPropagation()}
              style={metaChip(accent, true)}
            >
              {sup ? <Replace size={10} strokeWidth={2.2} aria-hidden style={{ flexShrink: 0 }} /> : <ArrowUpRight size={11} strokeWidth={2.4} aria-hidden style={{ flexShrink: 0 }} />}
              <span style={{ fontWeight: 700 }}>{link.rel}</span>
              {deep && <span style={{ fontFamily: "var(--ct-font-mono)", fontSize: "0.86em", opacity: 0.55 }}>{shortId(link.to)}</span>}
            </span>
          );
        })}
      </div>
    </div>
  );
}

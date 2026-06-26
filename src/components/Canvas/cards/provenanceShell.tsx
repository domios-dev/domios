import type { CSSProperties, ReactNode } from "react";
import { atLeast, World } from "../worlds";

// §26 shared shell for provenance artifacts (note, decision). A fixed-footprint
// card that fills its layout box; eyebrow→title header follows the TaskCard /
// SectionNode §header pattern (label above, title owns its own line). The body
// region clamps at far bands and scrolls node-internally at W3 — so the deep
// dive reveals everything untruncated. The card itself is always overflow:hidden,
// so nothing escapes the footprint regardless of band.

export function provenanceCardStyle(accent?: string): CSSProperties {
  return {
    position: "absolute",
    boxSizing: "border-box",
    border: "1px solid var(--ct-border-default)",
    ...(accent ? { borderLeft: `3px solid ${accent}` } : null),
    borderRadius: "var(--ct-radius-md)",
    background: "var(--ct-bg)",
    color: "var(--ct-fg)",
    fontFamily: "var(--ct-font-ui)",
    boxShadow: "var(--ct-shadow-sm)",
    display: "flex",
    flexDirection: "column",
    overflow: "hidden",
  };
}

export function Eyebrow({
  icon,
  label,
  accent,
  trailing,
  testid,
}: {
  icon: ReactNode;
  label: string;
  accent: string;
  trailing?: ReactNode;
  testid?: string;
}) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0, flexShrink: 0 }}>
      <span aria-hidden style={{ display: "inline-flex", alignItems: "center", color: accent, flexShrink: 0 }}>
        {icon}
      </span>
      <span
        data-testid={testid}
        style={{ fontSize: "0.62em", fontWeight: 800, letterSpacing: "0.13em", textTransform: "uppercase", color: accent, whiteSpace: "nowrap" }}
      >
        {label}
      </span>
      <span style={{ flex: 1, minWidth: 0 }} />
      {trailing}
    </div>
  );
}

// Clamp helper: a `-webkit-box` line clamp for the far bands, `block` when deep.
export function clampStyle(lines: number | null): CSSProperties {
  return lines != null
    ? { display: "-webkit-box", WebkitLineClamp: lines, WebkitBoxOrient: "vertical", overflow: "hidden" }
    : { display: "block" };
}

// The body region: scrolls at W3 (full reveal), clipped to the footprint below it.
// `min-height: 0` lets it shrink inside the flex column; the card clips the rest.
export function bodyRegionStyle(world: World): CSSProperties {
  const deep = atLeast(world, World.W3);
  return {
    flex: 1,
    minHeight: 0,
    display: "flex",
    flexDirection: "column",
    gap: 6,
    overflowY: deep ? "auto" : "hidden",
  };
}

export function firstString(obj: Record<string, unknown>, keys: string[]): string {
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === "string" && v.trim()) return v;
  }
  return "";
}

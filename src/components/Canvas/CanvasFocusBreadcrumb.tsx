import { Home } from "lucide-react";
import type { NavEntry } from "./canvasView/useCanvasNavHistory";

interface CanvasFocusBreadcrumbProps {
  /** Home base: the plan. Always the first crumb + the ⌂ button target. */
  planLabel: string;
  /** Hops away from home, in order — the real path you flew (plan ▸ section ▸ task …). */
  path: NavEntry[];
  /** Fly home (frame the plan) — clears the path. */
  onHome: () => void;
  /** Fly to crumb `index` in `path` (drops everything after it). */
  onCrumb: (index: number) => void;
  /** Back one hop (Esc). */
  onBack: () => void;
}

const crumbBtn: React.CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--ct-fg)",
  cursor: "pointer",
  padding: "0 2px",
  maxWidth: 180,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  font: "inherit",
};

// Return-to-plan navigation: the PLAN is home base. However deep you've flown, the
// ⌂ button frames the plan and Esc/Back pops one hop — and every crumb is clickable
// to fly straight to that hop. Hidden when already at the plan (empty path).
export function CanvasFocusBreadcrumb({ planLabel, path, onHome, onCrumb, onBack }: CanvasFocusBreadcrumbProps) {
  if (path.length === 0) return null;
  return (
    <div
      data-testid="canvas-breadcrumb"
      className="absolute top-2 left-2 z-20 flex items-center gap-1 rounded px-2 py-1 text-xs"
      style={{ background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)", border: "1px solid var(--ct-border-default)", color: "var(--ct-fg)", backdropFilter: "blur(8px)", maxWidth: "60vw" }}
    >
      <button
        type="button"
        data-testid="canvas-breadcrumb-home"
        title="Return to plan (home)"
        onClick={onHome}
        style={{ ...crumbBtn, display: "inline-flex", alignItems: "center", gap: 4, fontWeight: 700, maxWidth: 200 }}
      >
        <Home size={12} aria-hidden />
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 150 }}>{planLabel}</span>
      </button>
      {path.map((entry, i) => (
        <span key={`${entry.id}-${i}`} style={{ display: "inline-flex", alignItems: "center", gap: 1, minWidth: 0 }}>
          <span aria-hidden style={{ opacity: 0.5 }}>▸</span>
          <button
            type="button"
            data-testid="canvas-breadcrumb-crumb"
            data-nav-kind={entry.kind}
            title={`Fly to ${entry.label}`}
            onClick={() => onCrumb(i)}
            style={{ ...crumbBtn, fontWeight: i === path.length - 1 ? 700 : 500, opacity: i === path.length - 1 ? 1 : "var(--ct-text-secondary-opacity)" }}
          >
            {entry.label}
          </button>
        </span>
      ))}
      <button
        type="button"
        data-testid="canvas-breadcrumb-back"
        title="Back (Esc)"
        onClick={onBack}
        style={{ marginLeft: 4, padding: "0 6px", borderRadius: 4, border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer" }}
      >
        Esc
      </button>
    </div>
  );
}

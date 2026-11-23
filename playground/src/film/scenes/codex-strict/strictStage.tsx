import type { CSSProperties, ReactNode } from "react";
import { Camera, range, useStage } from "../../engine";

export const STRICT_GRID = {
  unit: 24,
  landscape: { width: 1920, height: 1080, margin: 96, end: 1800, footer: 984 },
  portrait: { width: 1080, height: 1920, margin: 108, end: 972, footer: 1440 },
};

const palette = {
  "--ct-bg": "#141414",
  "--ct-fg": "#f2f2f2",
  "--ct-surface": "#1c1c1c",
  "--ct-bg-secondary": "#1c1c1c",
  "--ct-bg-hover": "#262626",
  "--ct-border": "#3c3c3c",
  "--ct-muted": "#999999",
  "--ct-muted-fg": "#b0b0b0",
  "--ct-sidebar-bg": "#141414",
  "--ct-titlebar-bg": "#141414",
  "--ct-window-close": "#777777",
  "--ct-window-minimize": "#777777",
  "--ct-window-maximize": "#777777",
} as CSSProperties;

export function StrictStage({ t, number, label, children }: { t: number; number: string; label: string; children: ReactNode }) {
  const { portrait } = useStage();
  const grid = portrait ? STRICT_GRID.portrait : STRICT_GRID.landscape;
  return <div className="cx-strict" style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)", ...palette }}>
    <style>{`
      .cx-strict *, .cx-strict *::before, .cx-strict *::after { transition: none !important; animation: none !important; }
      .cx-strict .strict-memory .ct-jobs-header { display: none; }
      .cx-strict .strict-memory [data-testid="memory-list-sort"] { display: none; }
      .cx-strict .strict-memory [data-testid="memory-list-search"] { font-size: 12px; }
      .cx-strict .strict-memory [data-testid="memory-list-item"] .truncate { white-space: normal; }
      .cx-strict .strict-team [data-testid="sidebar-new-session"] { display: none; }
    `}</style>
    <Camera t={t} width={grid.width} height={grid.height} keys={[
      { t: 0, x: grid.width / 2, y: grid.height / 2, scale: 1 },
      { t: 8, x: grid.width / 2, y: grid.height / 2 + 24, scale: 1 },
    ]}>
      <div style={{ position: "absolute", left: grid.margin, top: 96, width: grid.end - grid.margin, display: "flex", alignItems: "baseline", fontSize: 26, letterSpacing: "0.06em" }}>
        <span style={{ width: portrait ? 120 : 144, color: "var(--ct-accent)" }}>{number}</span>
        <span>{label}</span>
        <span style={{ marginLeft: "auto", color: "var(--ct-muted-fg)", fontSize: 24 }}>DOMIOS</span>
      </div>
      {children}
      <div style={{ position: "absolute", left: grid.margin, top: grid.footer - 24, width: grid.end - grid.margin, height: 2, background: "var(--ct-border)" }}>
        <div style={{ height: 2, width: `${100 * range(t, 0, 8, (k) => k)}%`, background: "var(--ct-accent)" }} />
      </div>
      <div style={{ position: "absolute", left: grid.margin, top: grid.footer, color: "var(--ct-muted-fg)", fontSize: 26 }}>An operating system for agents.</div>
    </Camera>
  </div>;
}

export function StrictTitle({ lines }: { lines: string[] }) {
  const { portrait } = useStage();
  return <h1 style={{ position: "absolute", left: portrait ? 108 : 96, top: portrait ? 216 : 240, margin: 0, fontSize: portrait ? 120 : 144, lineHeight: 1, letterSpacing: "-0.065em", fontWeight: 500 }}>
    {lines.map((line, i) => <span key={line} style={{ display: "block", color: i === lines.length - 1 ? "var(--ct-accent)" : "var(--ct-fg)" }}>{line}</span>)}
  </h1>;
}

export function ProductPlate({ x, y, width, height, scale, children, className }: { x: number; y: number; width: number; height: number; scale: number; children: ReactNode; className?: string }) {
  return <div className={className} style={{ position: "absolute", left: x, top: y, width, height }}>
    <div style={{ position: "absolute", inset: 0, overflow: "hidden", filter: "grayscale(1)", border: "1px solid var(--ct-border)", background: "var(--ct-bg)" }}>
      <div style={{ position: "relative", width: width / scale, height: height / scale, transform: `scale(${scale})`, transformOrigin: "0 0" }}>{children}</div>
    </div>
  </div>;
}

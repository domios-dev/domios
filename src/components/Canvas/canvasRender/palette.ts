
export interface CanvasPalette {
  bg: string;
  cardBg: string;
  cardBgElev: string;
  border: string;
  borderStrong: string;
  fg: string;
  fgMuted: string;
  chipBg: string;
  accent: string;
}

const FALLBACK: CanvasPalette = {
  bg: "#0e1116",
  cardBg: "#171b22",
  cardBgElev: "#1d2230",
  border: "rgba(148,163,184,0.28)",
  borderStrong: "rgba(129,140,248,0.85)",
  fg: "#e6e9ef",
  fgMuted: "rgba(230,233,239,0.6)",
  chipBg: "rgba(148,163,184,0.16)",
  accent: "rgba(129,140,248,0.92)",
};

function readVar(style: CSSStyleDeclaration, name: string, fallback: string): string {
  const v = style.getPropertyValue(name).trim();
  return v || fallback;
}

export function resolveCanvasPalette(root?: HTMLElement | null): CanvasPalette {
  if (typeof window === "undefined" || !root) return FALLBACK;
  const s = window.getComputedStyle(root);
  return {
    bg: readVar(s, "--ct-bg", FALLBACK.bg),
    cardBg: readVar(s, "--ct-surface", FALLBACK.cardBg),
    cardBgElev: readVar(s, "--ct-bg-elev", FALLBACK.cardBgElev),
    border: readVar(s, "--ct-border", readVar(s, "--ct-border-default", FALLBACK.border)),
    borderStrong: readVar(s, "--ct-border-focus", FALLBACK.borderStrong),
    fg: readVar(s, "--ct-fg", FALLBACK.fg),
    fgMuted: readVar(s, "--ct-muted-fg", FALLBACK.fgMuted),
    chipBg: readVar(s, "--ct-bg-hover", FALLBACK.chipBg),
    accent: readVar(s, "--ct-accent", FALLBACK.accent),
  };
}

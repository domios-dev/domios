import type { CSSProperties, ReactNode } from "react";
import { easeOut, range, useStage } from "../../engine";

export const ink = "var(--ct-fg)";
export const accent = "var(--ct-accent)";
export const muted = "var(--ct-muted-fg)";

export function Frame({ children }: { children: ReactNode }) {
  return <div style={{ position: "absolute", inset: 0, overflow: "hidden", background: "var(--ct-bg)", color: ink, fontFamily: "Inter, sans-serif" }}>{children}</div>;
}

export function Slug({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return <div style={{ position: "absolute", fontSize: 28, fontWeight: 500, letterSpacing: ".16em", textTransform: "uppercase", color: muted, ...style }}>{children}</div>;
}

export function Slam({ t, at, children, size, style }: { t: number; at: number; children: ReactNode; size: number; style?: CSSProperties }) {
  const { portrait } = useStage();
  if (t < at) return null;
  const k = range(t, at, at + 0.24, easeOut);
  return <div style={{ fontSize: size, lineHeight: .87, letterSpacing: "-.065em", fontWeight: 850, transformOrigin: portrait ? "50% 50%" : "0 50%", transform: `translateY(${(1 - k) * -48}px) scale(${1 + (1 - k) * (portrait ? .1 : .38)})`, ...style }}>{children}</div>;
}

export function Rule({ t, at, left, top, width }: { t: number; at: number; left: number; top: number; width: number }) {
  return <div style={{ position: "absolute", left, top, width: width * range(t, at, at + .35, easeOut), height: 4, background: accent }} />;
}

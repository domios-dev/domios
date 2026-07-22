import "./living-dots.css";

export type LivingDotsSize = "micro" | "small" | "medium";
export type LivingDotsTone = "accent" | "thinking" | "muted";
export type LivingDotsRhythm = "work" | "thinking" | "streaming";

export interface LivingDotsProps {
  size?: LivingDotsSize;
  tone?: LivingDotsTone;
  rhythm?: LivingDotsRhythm;
  idle?: boolean;
}

export function LivingDots({ size = "medium", tone = "accent", rhythm = "work", idle = false }: LivingDotsProps) {
  return <span className={`ds-living-dots ds-living-dots--${size} ds-living-dots--${tone} ds-living-dots--${rhythm}${idle ? " ds-living-dots--idle" : ""}`} aria-hidden="true"><i /><i /><i /></span>;
}

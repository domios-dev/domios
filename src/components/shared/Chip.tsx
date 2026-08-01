
import React from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";

export type ChipVariant = "default" | "accent" | "success" | "danger" | "warning" | "muted";

export interface ChipProps {
  variant?: ChipVariant;
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
  "data-testid"?: string;
}

const VARIANT_EXTRA: Record<ChipVariant, string> = {
  default: "",
  accent: "border-[var(--ct-accent)]/40 text-[var(--ct-accent)]",
  success: "border-[var(--ct-green)]/40 text-[var(--ct-green)]",
  danger: "border-[var(--ct-red)]/40 text-[var(--ct-red)]",
  warning: "border-[var(--ct-yellow,#d97706)]/40 text-[var(--ct-yellow,#d97706)]",
  muted: "border-transparent text-[var(--ct-muted)]",
};

export function Chip({
  variant = "default",
  children,
  className,
  style,
  "data-testid": testId,
}: ChipProps) {
  const touchMode = useTouchOptimizedUi();
  const extra = VARIANT_EXTRA[variant];
  const cls = ["ct-small-pill", "ct-liquid-interactive", extra, className].filter(Boolean).join(" ");
  return (
    <span className={cls} style={style} data-touch-mode={touchMode ? "true" : undefined} data-testid={testId}>
      {children}
    </span>
  );
}

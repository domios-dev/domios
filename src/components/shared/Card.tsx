
import React from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";

export type CardVariant = "default" | "soft";

export interface CardProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: CardVariant;
  interactive?: boolean;
  children: React.ReactNode;
  onClick?: React.MouseEventHandler<HTMLDivElement>;
  "data-testid"?: string;
}

const VARIANT_CLASS: Record<CardVariant, string> = {
  default: "ct-panel-card",
  soft: "ct-panel-card ct-panel-card--soft",
};

export function Card({
  variant = "default",
  interactive = false,
  children,
  className,
  style,
  onClick,
  "data-testid": testId,
  ...rest
}: CardProps) {
  const touchMode = useTouchOptimizedUi();
  const interactiveCls = interactive
    ? "ct-liquid-interactive cursor-pointer transition-colors hover:border-[var(--ct-accent)]/50 hover:bg-[var(--ct-hover)]"
    : "";
  const cls = [VARIANT_CLASS[variant], interactiveCls, className].filter(Boolean).join(" ");
  return (
    <div
      {...rest}
      className={cls}
      style={style}
      onClick={onClick}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      data-touch-mode={interactive && touchMode ? "true" : undefined}
      onKeyDown={interactive && onClick ? (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick(e as unknown as React.MouseEvent<HTMLDivElement>);
        }
      } : undefined}
      data-testid={testId}
    >
      {children}
    </div>
  );
}

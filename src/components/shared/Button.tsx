
import React from "react";
import { BusyState } from "./BusyState";

export type ButtonVariant = "primary" | "ghost" | "accent" | "danger" | "icon";
export type ButtonSize = "sm" | "md" | "lg" | "compact";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  "data-testid"?: string;
}

const VARIANT_CLASS: Record<ButtonVariant, string> = {
  primary: "ct-btn ct-btn-primary",
  ghost: "ct-btn ct-btn-ghost",
  accent: "ct-btn ct-btn-accent",
  danger: "ct-btn ct-btn-danger",
  icon: "ct-btn ct-btn-icon ct-btn-ghost",
};

const SIZE_CLASS: Record<ButtonSize, string> = {
  sm: "ct-btn-sm",
  md: "",
  lg: "ct-btn-lg",
  compact: "ct-btn-compact",
};

export function Button({
  variant = "ghost",
  size = "md",
  loading = false,
  disabled,
  type = "button",
  children,
  className,
  "data-testid": testId,
  ...rest
}: ButtonProps) {
  const variantCls = VARIANT_CLASS[variant];
  const sizeCls = SIZE_CLASS[size];
  const cls = [variantCls, "ct-liquid-interactive", sizeCls, className].filter(Boolean).join(" ");

  return (
    <button
      {...rest}
      type={type}
      className={cls}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      data-testid={testId}
    >
      {loading ? (
        <>
          <BusyState inline hideLabel phase="work" aria-hidden="true" />
          {children}
        </>
      ) : (
        children
      )}
    </button>
  );
}

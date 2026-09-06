import { useCallback, useState, type ButtonHTMLAttributes, type ReactNode } from "react";

const VARIANTS = {
  primary: "ct-btn ct-btn-primary",
  ghost: "ct-btn ct-btn-ghost",
  accent: "ct-btn ct-btn-accent",
  danger: "ct-btn ct-btn-danger",
} as const;

const SIZES = {
  sm: "ct-btn-sm",
  md: "",
  lg: "ct-btn-lg",
} as const;

type Props = {
  variant?: keyof typeof VARIANTS;
  size?: keyof typeof SIZES;
  children: ReactNode;
} & ButtonHTMLAttributes<HTMLButtonElement>;

// The squash-jump lives on release: pointerdown gives the CSS :active squash, pointerup arms the one-shot jump keyframe, cleared on animationend so it can re-fire on the next press.
export function GlassButton({ variant = "primary", size = "md", children, className = "", ...rest }: Props) {
  const [releasing, setReleasing] = useState(false);
  const arm = useCallback(() => setReleasing(true), []);
  const disarm = useCallback(() => setReleasing(false), []);

  const classes = [
    VARIANTS[variant],
    SIZES[size],
    "lg-btn",
    releasing ? "lg-btn--release" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      type="button"
      className={classes}
      onPointerUp={arm}
      onAnimationEnd={disarm}
      {...rest}
    >
      <span className="lg-btn__label">{children}</span>
    </button>
  );
}

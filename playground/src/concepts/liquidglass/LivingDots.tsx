type Props = {
  variant?: "signature" | "glass" | "thinking";
  idle?: boolean;
  label?: string;
};

// The three-dot squash-jump signature, reusable. Markup mirrors the production
// ct-phase-dots contract (three <i> beads); styling comes from liquidglass.css.
export function LivingDots({ variant = "signature", idle = false, label }: Props) {
  const classes = [
    "lg-dots",
    variant === "glass" ? "lg-dots--glass" : "",
    variant === "thinking" ? "lg-dots--thinking" : "",
    idle ? "lg-dots--idle" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span className={classes} role="img" aria-label={label ?? (idle ? "idle" : "working")}>
      <i />
      <i />
      <i />
    </span>
  );
}

import type { FirstRunPhase } from "../../lib/firstRunFlow";

const steps = [
  ["installing", "Install"],
  ["starting", "Start General"],
  ["ready", "Ready"],
] as const;

export function FirstRunSetupProgress({ phase }: { phase: FirstRunPhase }) {
  if (phase === "choosing" || phase === "failed") return null;
  return <ol aria-label="Setup steps" className="flex flex-wrap gap-4 text-sm">
    {steps.filter(([step]) => step !== "installing" || phase === "installing").map(([step, label]) => <li key={step} aria-current={phase === step ? "step" : undefined}
      className={phase === step ? "font-semibold text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]"}>{label}</li>)}
  </ol>;
}

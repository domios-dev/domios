import { CheckCircle2, Circle, CircleAlert } from "lucide-react";
import { BusySpinner } from "./BusyState";

export type ComponentState = "available" | "active" | "queued" | "completed" | "failed" | "installed" | "marketplace";

export function ComponentStatus({ state, label }: { state: ComponentState; label: string }) {
  const Icon = state === "failed" ? CircleAlert : state === "completed" || state === "installed" ? CheckCircle2 : Circle;
  return <span role="status" aria-label={label} title={label} className={`inline-flex shrink-0 items-center ${state === "failed" ? "text-[var(--ct-red)]" : state === "completed" || state === "installed" ? "text-[var(--ct-green)]" : state === "active" || state === "available" ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted-fg)]"}`}>
    {state === "active" ? <BusySpinner size={13} /> : <Icon size={13} aria-hidden="true" />}
  </span>;
}

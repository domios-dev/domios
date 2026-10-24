import { LockKeyhole } from "lucide-react";

export function BlueprintPlanLock({ reason }: { reason?: string | null }) {
  if (reason !== "plan_limit_locked") return null;
  return <span role="status" aria-label="Blueprint locked by plan" title="plan_limit_locked" className="inline-flex items-center gap-1 text-xs text-[var(--ct-muted-fg)]">
    <LockKeyhole size={12} aria-hidden="true" />Locked by plan
  </span>;
}

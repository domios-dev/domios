import type { ProviderSwitchStatus } from "./tabProviderMenu";

export interface ProviderSwitchStatusPhase {
  phase: string;
  error: string | null;
}

export type ProviderSwitchStatusView =
  | { kind: "busy"; label: string }
  | { kind: "complete"; label: string }
  | { kind: "failed"; reason: string }
  | { kind: "unconfirmed"; reason: string };

const terminalPhases = new Set(["completed", "failed"]);

function phaseRank(phase: string): number {
  if (phase === "pending") return 0;
  if (terminalPhases.has(phase)) return 2;
  return 1;
}

export function preferProviderSwitchStatus(
  current: ProviderSwitchStatus | undefined,
  next: ProviderSwitchStatus,
): ProviderSwitchStatus {
  if (!current || current.operation_id !== next.operation_id) return next;
  if (terminalPhases.has(current.phase)) return current;
  if (terminalPhases.has(next.phase)) return next;
  if (phaseRank(current.phase) > phaseRank(next.phase)) return current;
  if (current.phase === next.phase && current.error === next.error) return current;
  return next;
}

export function providerSwitchStatusView(
  status: ProviderSwitchStatusPhase | null,
  targetName: string,
): ProviderSwitchStatusView | null {
  if (!status) return null;
  switch (status.phase) {
    case "pending": return { kind: "busy", label: `Starting ${targetName}…` };
    case "running": return { kind: "busy", label: `Switching to ${targetName}…` };
    case "completed": return { kind: "complete", label: "Bound — continuing" };
    case "failed": return { kind: "failed", reason: status.error ?? "The provider switch failed." };
    case "unconfirmed": return { kind: "unconfirmed", reason: status.error ?? "The switch status could not be confirmed." };
    default: return { kind: "busy", label: `Switching to ${targetName}…` };
  }
}

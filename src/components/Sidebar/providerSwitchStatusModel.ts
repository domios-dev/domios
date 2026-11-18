export interface ProviderSwitchStatusPhase {
  phase: string;
  error: string | null;
}

export type ProviderSwitchStatusView =
  | { kind: "busy"; label: string }
  | { kind: "complete"; label: string }
  | { kind: "failed"; reason: string };

export function providerSwitchStatusView(
  status: ProviderSwitchStatusPhase | null,
  targetName: string,
): ProviderSwitchStatusView | null {
  if (!status) return null;
  switch (status.phase) {
    case "pending": return { kind: "busy", label: `Starting ${targetName}…` };
    case "running": return { kind: "busy", label: "Handing over context…" };
    case "completed": return { kind: "complete", label: "Bound — continuing" };
    case "failed": return { kind: "failed", reason: status.error ?? "The provider switch failed." };
    default: return { kind: "busy", label: `Switching to ${targetName}…` };
  }
}

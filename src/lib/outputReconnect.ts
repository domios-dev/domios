import type { StreamEndReason } from "./remoteChannelRecovery";

export interface OutputReconnectState {
  failures: number;
  connectedMs: number;
  reason: StreamEndReason | "abort";
  current: boolean;
  subscribed: boolean;
}

export function planOutputReconnect(state: OutputReconnectState): { failures: number; delayMs: number } | null {
  if (!state.current || !state.subscribed || state.reason === "abort" || state.reason === "refused") return null;
  const failures = state.connectedMs >= 30_000 ? 0 : state.failures;
  return { failures: Math.min(failures + 1, 8), delayMs: Math.min(250 * 2 ** failures, 30_000) };
}

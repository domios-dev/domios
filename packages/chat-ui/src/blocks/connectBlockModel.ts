import type { ChatBlockStatus } from "./ChatBlockFrame";

export type BridgePhase = "connected" | "connecting" | "failing" | "disconnected";

/** How a bridge is connected in place; a QR or pairing step joins this union when a bridge needs one. */
export interface TokenConnectMethod {
  kind: "token";
  label: string;
  placeholder: string;
  help: string;
}

/** Facts the host reports about one messenger bridge; the block derives everything it shows from these. */
export interface BridgeConnectInfo {
  id: string;
  name: string;
  description: string;
  phase: BridgePhase;
  /** Who the bridge acts as, e.g. "bot 123456789"; null when unknown. */
  identity?: string | null;
  tokenStored: boolean;
  error?: string | null;
  method: TokenConnectMethod;
}

export type ConnectBlockState =
  | { kind: "loading" }
  | { kind: "unavailable"; message: string }
  | { kind: "unsupported" }
  | { kind: "ready"; bridge: BridgeConnectInfo };

const BRIDGE_ID = /^[a-z][a-z0-9-]{0,31}$/;

export function parseBridgeId(argument: string): string | null {
  const id = argument.trim();
  return BRIDGE_ID.test(id) ? id : null;
}

export function connectStatus(phase: BridgePhase): ChatBlockStatus {
  switch (phase) {
    case "connected":
      return { label: "Connected", tone: "ok" };
    case "connecting":
      return { label: "Connecting", tone: "accent" };
    case "failing":
      return { label: "Not responding", tone: "warn" };
    case "disconnected":
      return { label: "Not connected", tone: "muted" };
  }
}

/** Whether the token field shows: always without a stored token, otherwise only when the user asks to replace it. */
export function needsTokenEntry(bridge: BridgeConnectInfo, replacing: boolean): boolean {
  return bridge.phase !== "connected" && (!bridge.tokenStored || replacing);
}

/** The in-place result line after Connect, judged against the state the host reports afterwards. */
export function connectResult(after: ConnectBlockState): string {
  if (after.kind !== "ready") return "Connect finished, but the bridge state is unknown. Open Messengers settings to check.";
  const { bridge } = after;
  switch (bridge.phase) {
    case "connected":
      return bridge.identity ? `Connected as ${bridge.identity}.` : "Connected.";
    case "connecting":
      return `Started. Waiting for ${bridge.name} to answer.`;
    case "failing":
      return `Started, but ${bridge.name} is not answering${bridge.error ? `: ${bridge.error}` : "."}`;
    case "disconnected":
      return `${bridge.name} did not start. Check the token and try again.`;
  }
}

export type ConnectionPhase = import("../../../gen/ConnectionPhase").ConnectionPhase;
export type PeerConnection = import("../../../gen/PeerConnection").PeerConnection;

export function connectionPresentation(phase?: ConnectionPhase): { connecting: boolean; label: string } {
  switch (phase) {
    case "discovering": return { connecting: true, label: "Connecting — locating machine" };
    case "connecting": return { connecting: true, label: "Connecting — opening transport" };
    case "verifying": return { connecting: true, label: "Connecting — verifying identity" };
    case "committing": return { connecting: true, label: "Connecting — completing handshake" };
    case "auth_refused": return { connecting: false, label: "Authentication refused — retry scheduled" };
    case "membership_expired": return { connecting: false, label: "Membership expired — waiting for renewal" };
    case "peer_needs_update": return { connecting: false, label: "Peer needs an update" };
    case "ready": return { connecting: false, label: "Connected" };
    case "failed": return { connecting: false, label: "Unreachable — connection failed" };
    case "retrying": return { connecting: false, label: "Unreachable — retry scheduled" };
    default: return { connecting: false, label: "Waiting for a connection attempt" };
  }
}

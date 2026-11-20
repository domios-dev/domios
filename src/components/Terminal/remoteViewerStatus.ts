import type { TabStatus } from "../../lib/types";
import { REMOTE_CHAT_COPY, type RemoteChatState } from "../../lib/remoteChatState";
import type { RemoteTransportAvailability } from "../../lib/remoteTabTransportState";
import type { BrowsedWorkspace, GeneralAvailability, PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";

export type RemoteViewerConnectionState = RemoteChatState | "reconnecting";

export interface RemoteViewerObservation {
  peerId: string;
  name: string;
  availability: GeneralAvailability;
  status?: TabStatus;
}

export function remoteViewerObservation(
  generals: readonly PairedGeneral[],
  browse: { target: { id: string; name: string }; availability: GeneralAvailability; workspaces: readonly BrowsedWorkspace[] } | null,
  viewerId: string,
  peerId?: string | null,
): RemoteViewerObservation | undefined {
  for (const peer of generals) {
    const session = peer.sessions.find(item => item.viewerId === viewerId);
    if (session) return { peerId: peer.peerId, name: peer.name, availability: peer.availability, status: session.status };
  }

  const workspace = browse?.workspaces.find(item => item.tabs.some(tab => tab.viewerId === viewerId));
  const tab = workspace?.tabs.find(item => item.viewerId === viewerId);
  if (browse && workspace && tab) return { peerId: workspace.peerId, name: browse.target.name, availability: browse.availability, status: tab.status };

  const paired = generals.find(item => item.peerId === peerId);
  if (paired) return { peerId: paired.peerId, name: paired.name, availability: paired.availability };
  if (peerId && browse?.target.id === peerId) return { peerId, name: browse.target.name, availability: browse.availability };
  return undefined;
}

export function remoteViewerConnectionState(
  availability: GeneralAvailability | undefined,
  chatChannel: RemoteTransportAvailability | undefined,
  chatState: RemoteChatState | null,
): RemoteViewerConnectionState | null {
  if (availability === "offline" || availability === "signed-out") return availability;
  if (availability === "update-required") return "too-old";
  if (chatState && chatState !== "unreachable" && chatState !== "generic") return chatState;
  if (chatChannel === "unavailable") return "unreachable";
  if (chatChannel === "reconnecting" || availability === "reconnecting") return "reconnecting";
  if (availability === "unreachable") return "unreachable";
  if (chatState) return chatState;
  return null;
}

export function remoteViewerStatusLabel(state: RemoteViewerConnectionState | null, status?: TabStatus | null): string | null {
  if (state === "reconnecting") return state;
  if (state) return REMOTE_CHAT_COPY[state].label;
  if (status === "Waiting") return "waiting for you";
  return status?.toLowerCase() ?? null;
}

export function remoteViewerSendUnavailableReason(state: RemoteViewerConnectionState | null, machine: string): string | null {
  if (state === "reconnecting") return `${machine} is reconnecting`;
  return state ? REMOTE_CHAT_COPY[state].sendReason(machine) : null;
}

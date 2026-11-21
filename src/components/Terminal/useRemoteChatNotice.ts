import { useRef, useState } from "react";
import { readRemoteChatStatus, setRemoteChatFailure, useRemoteChatStatus } from "../../lib/remoteChatStatus";
import { REMOTE_CHAT_COPY, remoteChatFailure, remoteMachineName } from "../../lib/remoteChatState";
import type { Tab } from "../../lib/types";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { reconcileTrackedChatMessages } from "./chatSendStatus";
import type { DeliverySend } from "./chatDeliveryModel";
import type { RemoteViewerConnection } from "./useRemoteViewerConnection";

const disclosures = new Map<string, boolean>();

export function useRemoteChatNotice(tabId: string, tab: Tab | null | undefined, sends: readonly DeliverySend[], connection?: RemoteViewerConnection) {
  const fallbackPeerId = tab?.remote_peer_id ?? parseCompositeTabId(tabId)?.peerId;
  const fallback = useRemoteChatStatus(tabId, fallbackPeerId);
  const peer = connection ? connection.peer : fallback.peer;
  const failure = connection ? connection.failure : fallback.failure;
  const peerId = connection?.peerId ?? fallbackPeerId;
  const state = connection ? connection.state : fallback.state;
  const pending = peerId ? sends.filter(send => !send.dismissed && send.remoteFailure) : [];
  const delivery = pending[0]?.remoteFailure;
  const [, renderDisclosure] = useState(0);
  const expanded = disclosures.has(tabId);
  const [checking, setChecking] = useState(false);
  const checkInFlight = useRef(false);

  async function checkDelivery(resend: (id: string) => Promise<void>) {
    if (!peerId || checkInFlight.current) return;
    checkInFlight.current = true;
    setChecking(true);
    try {
      const statuses = await reconcileTrackedChatMessages(tabId);
      if (readRemoteChatStatus(tabId, peerId).failure === failure) setRemoteChatFailure(tabId);
      for (const send of pending) {
        if (send.remoteFailure?.neverWritten !== true || statuses.some(item => item.correlationId === send.correlationId)) continue;
        if (readRemoteChatStatus(tabId, peerId).state) break;
        await resend(send.id);
      }
    } catch (error) {
      setRemoteChatFailure(tabId, remoteChatFailure(error));
    } finally {
      checkInFlight.current = false;
      setChecking(false);
    }
  }

  return {
    peerId,
    blocked: peer?.presence != null && peer.presence !== "online",
    state: state ?? delivery?.state ?? null,
    connectionState: connection ? connection.connectionState : tab?.remote_channels?.chat === "reconnecting" ? "reconnecting" : null,
    sendUnavailableReason: connection ? connection.sendUnavailableReason : state ? REMOTE_CHAT_COPY[state].sendReason(remoteMachineName(peerId ?? "", peer?.name, tab?.remote_peer_name)) : null,
    failure: failure ?? delivery,
    machine: connection ? connection.machine : remoteMachineName(peerId ?? "", peer?.name, tab?.remote_peer_name),
    canRetrySend: Boolean(delivery && !(connection ? connection.connectionState : state)),
    pendingSend: Boolean(delivery),
    checking,
    expanded,
    toggle: () => {
      disclosures.delete(tabId);
      if (!expanded) {
        if (disclosures.size >= 128) disclosures.delete(disclosures.keys().next().value!);
        disclosures.set(tabId, true);
      }
      renderDisclosure(value => value + 1);
    },
    checkDelivery,
  };
}

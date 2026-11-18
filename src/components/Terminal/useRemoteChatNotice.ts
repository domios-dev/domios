import { useRef, useState } from "react";
import { readRemoteChatStatus, setRemoteChatFailure, useRemoteChatStatus } from "../../lib/remoteChatStatus";
import { remoteChatFailure, remoteMachineName } from "../../lib/remoteChatState";
import type { Tab } from "../../lib/types";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { reconcileTrackedChatMessages } from "./chatSendStatus";
import type { DeliverySend } from "./chatDeliveryModel";

const disclosures = new Map<string, boolean>();

export function useRemoteChatNotice(tabId: string, tab: Tab | null | undefined, sends: readonly DeliverySend[]) {
  const peerId = tab?.remote_peer_id ?? parseCompositeTabId(tabId)?.peerId;
  const { peer, failure, state } = useRemoteChatStatus(tabId, peerId);
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
    failure: failure ?? delivery,
    machine: remoteMachineName(peerId ?? "", peer?.name, tab?.remote_peer_name),
    canRetrySend: Boolean(delivery && !state),
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

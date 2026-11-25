import { classifyTabTarget, parseCompositeTabId } from "./remoteTabTransport";

export interface ProviderCliTarget {
  peerId?: string;
  callerTabId?: string;
}

/**
 * Resolves the owning machine and daemon-local caller tab for a provider CLI
 * read or mutation. Composite `mesh:<peer>/<tab>` ids always bind to that peer
 * and strip the composite prefix so the peer receives its own tab id.
 */
export function resolveProviderCliTarget(input: {
  callerTabId?: string;
  peerId?: string | null;
}): ProviderCliTarget {
  const { callerTabId, peerId } = input;
  const tabTarget = classifyTabTarget(callerTabId);
  if (tabTarget.kind === "invalid-remote") {
    throw new Error(`Invalid remote tab id "${tabTarget.value}"; local fallback is forbidden`);
  }
  const composite = parseCompositeTabId(callerTabId);
  if (composite) {
    if (typeof peerId === "string" && peerId && peerId !== composite.peerId) {
      throw new Error(`Provider CLI target peer "${peerId}" does not match tab peer "${composite.peerId}"`);
    }
    return { peerId: composite.peerId, callerTabId: composite.remoteTabId };
  }
  if (typeof peerId === "string" && !peerId.trim()) {
    throw new Error("An exact peer installation id is required");
  }
  return {
    peerId: typeof peerId === "string" ? peerId : undefined,
    callerTabId,
  };
}

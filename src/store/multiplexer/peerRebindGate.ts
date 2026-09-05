/**
 * The write gate means: we hold an accepted snapshot of the current origin.
 *
 * A peer switch locks it and the rebind's own refresh normally unlocks it. But
 * the daemon can refuse a snapshot under load (503), and a refusal is not-yet,
 * not failure -- the origin is fine, the request was declined. So a refused
 * rebind stays OPEN for its switch generation and unlocks on the first
 * accepted snapshot for that same generation, whatever event produces it.
 *
 * The rebind itself is never retried and nothing is scheduled: the next
 * ordinary `mux-state-changed` refresh is what closes it. A later peer switch
 * supersedes an open rebind, which then never unlocks -- its origin is gone.
 */
export interface PeerRebindGate {
  /** A peer switch is rebinding; hold the gate open for this generation. */
  open(switchId: number): void;
  /** An accepted snapshot landed while `currentSwitchId` is the live one. */
  acceptedSnapshot(currentSwitchId: number): void;
  /** The rebind refresh was refused. Stay open; a later snapshot will do it. */
  refused(switchId: number): void;
  /** The rebind refresh failed outright. Close without unlocking. */
  failed(switchId: number): void;
  openFor(): number | null;
}

export interface PeerRebindGateHandlers {
  unlock(): void;
  announce(switchId: number, success: boolean): void;
}

export function createPeerRebindGate(handlers: PeerRebindGateHandlers): PeerRebindGate {
  let openSwitchId: number | null = null;

  return {
    open(switchId) {
      openSwitchId = switchId;
    },
    acceptedSnapshot(currentSwitchId) {
      if (openSwitchId === null || openSwitchId !== currentSwitchId) return;
      openSwitchId = null;
      handlers.unlock();
      handlers.announce(currentSwitchId, true);
    },
    refused(switchId) {
      // Deliberately nothing: the gate stays open for this generation.
      if (openSwitchId !== switchId) return;
    },
    failed(switchId) {
      if (openSwitchId !== switchId) return;
      openSwitchId = null;
      handlers.announce(switchId, false);
    },
    openFor: () => openSwitchId,
  };
}

import { listen } from "../../lib/bridge";
import { SharedRead } from "../../lib/sharedRead";
import type { NeighborClient } from "./neighborClient";
import { parseNeighbors, type NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";

const clients = new WeakMap<NeighborClient, Map<string, NeighborReads>>();

class NeighborReads {
  readonly list = new SharedRead<NeighborList>();
  private readonly reloads = new Set<(read: Promise<NeighborList>) => void>();
  private stopReloads: (() => void) | undefined;
  private claim: Promise<ReferralClaim> | undefined;

  constructor(private readonly accounts: Map<string, NeighborReads>, private readonly identity: string) {}

  subscribeReloads(client: NeighborClient, listener: (read: Promise<NeighborList>) => void) {
    this.reloads.add(listener);
    if (!this.stopReloads) {
      let live = true;
      const stops: Array<() => void> = [];
      const reload = () => {
        if (!live) return;
        const read = this.list.read(() => client.list(), true);
        void read.catch(() => {});
        for (const notify of this.reloads) notify(read);
      };
      for (const event of ["entitlement-changed", "neighbor-daily-limit-changed", "neighbor-file-daily-limit-changed"]) {
        void listen(event, reload).then(stop => { if (live) stops.push(stop); else stop(); });
      }
      void listen<{ uid?: unknown; list?: unknown }>("neighbors-changed", event => {
        if (!live || event.payload?.uid !== this.identity) return;
        try { parseNeighbors(event.payload.list); }
        catch { reload(); }
      }).then(stop => { if (live) stops.push(stop); else stop(); });
      void listen("account-changed", () => {
        if (live && this.accounts.get(this.identity) === this) this.claim = undefined;
      }).then(stop => { if (live) stops.push(stop); else stop(); });
      this.stopReloads = () => { live = false; stops.forEach(stop => stop()); };
    }
    return () => {
      this.reloads.delete(listener);
      if (this.reloads.size === 0) { this.stopReloads?.(); this.stopReloads = undefined; }
    };
  }

  referral(client: NeighborClient): Promise<ReferralClaim> {
    this.claim ??= client.claimReferralTrial();
    return this.claim;
  }
}

export function neighborReads(client: NeighborClient, identity: string | null) {
  let accounts = clients.get(client);
  if (!accounts) { accounts = new Map(); clients.set(client, accounts); }
  if (!identity) { accounts.clear(); return null; }
  if (accounts.size > 0 && !accounts.has(identity)) accounts.clear();
  let reads = accounts.get(identity);
  if (!reads) { reads = new NeighborReads(accounts, identity); accounts.set(identity, reads); }
  return reads;
}

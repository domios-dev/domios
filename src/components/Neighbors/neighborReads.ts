import { listen } from "../../lib/bridge";
import { SharedRead } from "../../lib/sharedRead";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";

const clients = new WeakMap<NeighborClient, Map<string, NeighborReads>>();
const eligibility = (list: NeighborList) => JSON.stringify([list.access, list.can_invite, list.incoming.map(row => row.request_id).sort()]);

class NeighborReads {
  readonly list = new SharedRead<NeighborList>();
  private readonly reloads = new Set<(read: Promise<NeighborList>) => void>();
  private stopReloads: (() => void) | undefined;
  private eligibility: string | undefined;
  private claim: Promise<ReferralClaim> | undefined;
  private failure: { error: unknown; retryAt: number } | undefined;

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
      this.stopReloads = () => { live = false; stops.forEach(stop => stop()); };
    }
    return () => {
      this.reloads.delete(listener);
      if (this.reloads.size === 0) { this.stopReloads?.(); this.stopReloads = undefined; }
    };
  }

  referral(client: NeighborClient, force = false): Promise<ReferralClaim> {
    if (this.claim) return this.claim;
    if (!force && this.failure && Date.now() < this.failure.retryAt) return Promise.reject(this.failure.error);
    const flight = client.claimReferralTrial().catch((error: unknown) => {
      if (this.accounts.get(this.identity) === this && this.claim === flight) {
        this.claim = undefined;
        this.failure = { error, retryAt: Date.now() + 60_000 };
      }
      throw error;
    });
    this.claim = flight;
    return flight;
  }

  observe(list: NeighborList) {
    const next = eligibility(list);
    if (this.eligibility !== undefined && this.eligibility !== next) { this.claim = undefined; this.failure = undefined; }
    this.eligibility = next;
  }
}

export function neighborReads(client: NeighborClient, identity: string | null) {
  let accounts = clients.get(client);
  if (!accounts) { accounts = new Map(); clients.set(client, accounts); }
  if (!identity) { accounts.clear(); return null; }
  let reads = accounts.get(identity);
  if (!reads) { reads = new NeighborReads(accounts, identity); accounts.set(identity, reads); }
  return reads;
}

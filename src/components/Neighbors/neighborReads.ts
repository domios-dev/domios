import { SharedRead } from "../../lib/sharedRead";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";

const clients = new WeakMap<NeighborClient, Map<string, NeighborReads>>();
const eligibility = (list: NeighborList) => JSON.stringify([list.access, list.can_invite, list.incoming.map(row => row.request_id).sort()]);

class NeighborReads {
  readonly list = new SharedRead<NeighborList>();
  private eligibility: string | undefined;
  private claim: Promise<ReferralClaim> | undefined;

  referral(client: NeighborClient) {
    this.claim ??= client.claimReferralTrial();
    return this.claim;
  }

  observe(list: NeighborList) {
    const next = eligibility(list);
    if (this.eligibility !== undefined && this.eligibility !== next) this.claim = undefined;
    this.eligibility = next;
  }
}

export function neighborReads(client: NeighborClient, identity: string | null) {
  let accounts = clients.get(client);
  if (!accounts) { accounts = new Map(); clients.set(client, accounts); }
  if (!identity) { accounts.clear(); return null; }
  let reads = accounts.get(identity);
  if (!reads) { reads = new NeighborReads(); accounts.set(identity, reads); }
  return reads;
}

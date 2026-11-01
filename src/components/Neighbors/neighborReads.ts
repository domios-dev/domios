import { listen } from "../../lib/bridge";
import { SharedRead } from "../../lib/sharedRead";
import type { NeighborClient } from "./neighborClient";
import { parseNeighbors, type NeighborList } from "./neighborModel";

let clients = new WeakMap<NeighborClient, Map<string, NeighborReads>>();
const activeReads = new Set<NeighborReads>();

export function resetNeighborReads() {
  for (const reads of activeReads) reads.dispose();
  clients = new WeakMap();
}

class NeighborReads {
  readonly list = new SharedRead<NeighborList>(undefined, Infinity);
  private readonly reloads = new Set<(read: Promise<NeighborList>) => void>();
  private stopReloads: (() => void) | undefined;

  constructor(private readonly identity: string) {}

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
        try { this.list.write(parseNeighbors(event.payload.list)); }
        catch { reload(); }
      }).then(stop => { if (live) stops.push(stop); else stop(); });
      void listen("account-changed", reload).then(stop => { if (live) stops.push(stop); else stop(); });
      this.stopReloads = () => { live = false; stops.forEach(stop => stop()); };
    }
    return () => {
      this.reloads.delete(listener);
    };
  }

  dispose() { this.stopReloads?.(); activeReads.delete(this); }
}

export function neighborReads(client: NeighborClient, identity: string | null) {
  let accounts = clients.get(client);
  if (!accounts) { accounts = new Map(); clients.set(client, accounts); }
  if (!identity) { for (const reads of accounts.values()) reads.dispose(); accounts.clear(); return null; }
  if (accounts.size > 0 && !accounts.has(identity)) { for (const reads of accounts.values()) reads.dispose(); accounts.clear(); }
  let reads = accounts.get(identity);
  if (!reads) { reads = new NeighborReads(identity); accounts.set(identity, reads); activeReads.add(reads); }
  return reads;
}

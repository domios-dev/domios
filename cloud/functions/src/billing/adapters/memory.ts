import { reduceEntitlement, type BillingEvent, type Entitlement } from "../model";
import type { BillingStore } from "../service";
export class MemoryBillingStore implements BillingStore {
  customers = new Map<string,string>();
  entitlements = new Map<string,Entitlement>();
  events = new Set<string>();
  private pending = new Map<string,Promise<string>>();
  async customer(uid: string): Promise<string | undefined> { return this.customers.get(uid); }
  async ensureCustomer(uid: string, create: () => Promise<string>): Promise<string> {
    const existing = this.customers.get(uid);
    if (existing) return existing;
    const active = this.pending.get(uid);
    if (active) return active;
    const operation = create().then(customer => { this.customers.set(uid,customer); return customer; }).finally(() => this.pending.delete(uid));
    this.pending.set(uid,operation);
    return operation;
  }
  async apply(event: BillingEvent): Promise<boolean> {
    if (event.mode !== "test") throw new Error("Local billing only supports test mode");
    if (this.events.has(event.id)) return false;
    const uid = [...this.customers].find(([,customer]) => customer === event.customer)?.[0];
    if (event.change && !uid) throw new Error("Unknown customer");
    if (uid) {
      const next = reduceEntitlement(this.entitlements.get(uid),event);
      if (next) this.entitlements.set(uid,next);
    }
    this.events.add(event.id);
    return true;
  }
}

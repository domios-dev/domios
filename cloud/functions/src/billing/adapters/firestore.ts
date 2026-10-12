import type { Firestore } from "firebase-admin/firestore";
import { collectionName, reduceEntitlement, type BillingEvent, type BillingMode, type Entitlement } from "../model";
import type { BillingStore } from "../service";
export class FirestoreBillingStore implements BillingStore {
  private pending = new Map<string,Promise<string>>();
  constructor(private db: Firestore, private mode: BillingMode) {}
  private customers() { return this.db.collection(collectionName(this.mode,"billingCustomers")); }
  async customer(uid: string): Promise<string | undefined> { return (await this.customers().doc(uid).get()).data()?.customer; }
  async ensureCustomer(uid: string, create: () => Promise<string>): Promise<string> {
    const active = this.pending.get(uid);
    if (active) return active;
    const operation = this.persistCustomer(uid,create).finally(() => this.pending.delete(uid));
    this.pending.set(uid,operation);
    return operation;
  }
  private async persistCustomer(uid: string, create: () => Promise<string>): Promise<string> {
    const existing = await this.customer(uid);
    if (existing) return existing;
    const customer = await create();
    return this.db.runTransaction(async tx => {
      const ref = this.customers().doc(uid);
      const stored = (await tx.get(ref)).data();
      if (stored) return stored.customer as string;
      tx.create(ref,{customer});
      return customer;
    });
  }
  async apply(event: BillingEvent): Promise<boolean> {
    if (event.mode !== this.mode) throw new Error("Billing mode mismatch");
    return this.db.runTransaction(async tx => {
      const eventRef = this.db.doc(`${collectionName(this.mode,"stripeEvents")}/${event.id}`);
      if ((await tx.get(eventRef)).exists) return false;
      const matches = event.customer ? await tx.get(this.customers().where("customer","==",event.customer).limit(2)) : null;
      if (matches && matches.size > 1) throw new Error("Ambiguous customer owner");
      const uid = matches?.docs[0]?.id;
      const entitlementRef = uid ? this.db.doc(`${collectionName(this.mode,"entitlements")}/${uid}`) : null;
      const previous = entitlementRef ? (await tx.get(entitlementRef)).data() as Entitlement | undefined : undefined;
      if (event.change && !uid) throw new Error("Unknown billing customer");
      const next = reduceEntitlement(previous,event);
      tx.create(eventRef,{created:event.created});
      if (entitlementRef && next && next !== previous) tx.set(entitlementRef,next);
      return true;
    });
  }
}

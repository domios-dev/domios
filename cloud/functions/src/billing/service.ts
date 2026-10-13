import type { BillingEvent, BillingMode, PaymentGateway } from "./model";
export interface BillingConfig { mode: BillingMode; plans: Record<string, string>; hostedOrigin: string }
export interface BillingStore {
  customer(uid: string): Promise<string | undefined>;
  ensureCustomer(uid: string, create: () => Promise<string>): Promise<string>;
  apply(event: BillingEvent): Promise<boolean>;
}
export class BillingError extends Error {
  constructor(readonly code: "unauthenticated" | "invalid-argument" | "failed-precondition", message: string) { super(message); }
}
export class BillingService {
  constructor(readonly gateway: PaymentGateway, readonly store: BillingStore, readonly config: BillingConfig) {
    const url = new URL(config.hostedOrigin);
    if (url.protocol !== "https:" || url.origin !== config.hostedOrigin) throw new Error("Invalid hosted billing origin");
  }
  async checkout(uid: string | undefined, data: unknown): Promise<{url: string}> {
    if (!uid) throw new BillingError("unauthenticated", "Authentication required");
    if (!data || typeof data !== "object" || Array.isArray(data) || Object.keys(data).some(key => key !== "plan")
      || !("plan" in data) || typeof data.plan !== "string" || !Object.hasOwn(this.config.plans, data.plan)) {
      throw new BillingError("invalid-argument", "Unknown billing plan");
    }
    const price = this.config.plans[data.plan];
    const create = this.gateway.createCustomer?.bind(this.gateway);
    const customer = create
      ? await this.store.ensureCustomer(uid, () => create(uid, `billing:${this.config.mode}:${uid}`))
      : await this.store.customer(uid) ?? "";
    return {url: await this.gateway.createCheckout({uid,customer,price,successUrl:`${this.config.hostedOrigin}/billing/success`,cancelUrl:`${this.config.hostedOrigin}/billing/cancel`})};
  }
  async portal(uid: string | undefined): Promise<{url: string}> {
    if (!uid) throw new BillingError("unauthenticated", "Authentication required");
    const customer = await this.store.customer(uid);
    if (!customer) throw new BillingError("failed-precondition", "No billing customer");
    return {url:await this.gateway.createPortal(customer,`${this.config.hostedOrigin}/billing`)};
  }
  async webhook(body: Buffer, signature: string): Promise<boolean> {
    const event = await this.gateway.verifyAndParseWebhook(body, signature);
    if (event.mode !== this.config.mode) throw new Error("Billing mode mismatch");
    return this.store.apply(event);
  }
}

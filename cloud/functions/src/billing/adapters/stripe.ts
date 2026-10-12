import Stripe from "stripe";
import type { BillingEvent, BillingMode, BillingStatus, CheckoutInput, PaymentGateway } from "../model";

type ObjectData = Record<string, unknown>;
function object(value: unknown): ObjectData { return value && typeof value === "object" ? value as ObjectData : {}; }
function id(value: unknown): string | null { return typeof value === "string" ? value : typeof object(value).id === "string" ? object(value).id as string : null; }
function rows(value: unknown): ObjectData[] { const data = object(value).data; return Array.isArray(data) ? data.map(object) : []; }
const statuses = new Set<string>(["active","past_due","canceled","incomplete","trialing","unpaid","paused"]);
export class StripeGateway implements PaymentGateway {
  constructor(private client: Stripe, private secret: string, private mode: BillingMode, private plans: Record<string,string>) {}
  async createCustomer(uid: string, idempotencyKey: string): Promise<string> {
    return (await this.client.customers.create({metadata:{uid}},{idempotencyKey})).id;
  }
  async createCheckout(input: CheckoutInput): Promise<string> {
    const session = await this.client.checkout.sessions.create({mode:"subscription",customer:input.customer,
      line_items:[{price:input.price,quantity:1}],success_url:input.successUrl,cancel_url:input.cancelUrl,
      client_reference_id:input.uid,metadata:{uid:input.uid},subscription_data:{metadata:{uid:input.uid}}});
    if (!session.url) throw new Error("Checkout URL missing");
    return session.url;
  }
  async createPortal(customer: string, returnUrl: string): Promise<string> {
    return (await this.client.billingPortal.sessions.create({customer,return_url:returnUrl})).url;
  }
  private subscriptionChange(subscription: ObjectData): NonNullable<BillingEvent["change"]> {
    const items = rows(subscription.items);
    const plan = Object.entries(this.plans).find(([,price]) => price === id(items[0]?.price))?.[0];
    if (!plan || items.length !== 1 || !statuses.has(String(subscription.status))) throw new Error("Unsupported subscription");
    const end = items[0].current_period_end;
    if (typeof end !== "number") throw new Error("Missing subscription period");
    return {plan,status:subscription.status as BillingStatus,current_period_end:end,cancel_at_period_end:subscription.cancel_at_period_end === true};
  }
  async verifyAndParseWebhook(body: Buffer, signature: string): Promise<BillingEvent> {
    const event = this.client.webhooks.constructEvent(body,signature,this.secret);
    const mode = event.livemode ? "live" : "test";
    if (mode !== this.mode) throw new Error("Billing mode mismatch");
    const data = object(event.data.object);
    let customer = id(data.customer);
    let change: BillingEvent["change"] = null;
    if (event.type.startsWith("customer.subscription.")) {
      if (["customer.subscription.created","customer.subscription.updated","customer.subscription.deleted"].includes(event.type)) {
        change = this.subscriptionChange(data);
        if (event.type === "customer.subscription.deleted") change.status = "canceled";
      }
    } else if (event.type === "checkout.session.completed") {
      const subscriptionId = id(data.subscription);
      if (subscriptionId) {
        const subscription = await this.client.subscriptions.retrieve(subscriptionId);
        if (data.payment_status === "paid" || subscription.status === "active") change = this.subscriptionChange(object(subscription));
      }
    } else if (event.type === "invoice.paid" || event.type === "invoice.payment_failed") {
      const subscriptionId = id(object(object(data.parent).subscription_details).subscription);
      if (subscriptionId) {
        const subscription = await this.client.subscriptions.retrieve(subscriptionId);
        change = this.subscriptionChange(object(subscription));
        change.status = event.type === "invoice.paid" ? "active" : "past_due";
        if (event.type === "invoice.paid") {
          const ends = rows(data.lines).map(line => object(line.period).end).filter((end): end is number => typeof end === "number");
          if (ends.length) change.current_period_end = Math.max(...ends);
        }
      }
    } else if (event.type === "charge.refunded") {
      change = {status:"revoked"};
    } else if (event.type === "charge.dispute.created") {
      const chargeId = id(data.charge);
      if (!chargeId) throw new Error("Missing disputed charge");
      customer = id((await this.client.charges.retrieve(chargeId)).customer);
      change = {status:"revoked"};
    }
    return {id:event.id,mode,created:event.created,customer,change};
  }
}

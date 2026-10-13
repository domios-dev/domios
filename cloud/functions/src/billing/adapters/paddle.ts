import { createHmac, timingSafeEqual } from "node:crypto";
import type { BillingEvent, BillingMode, BillingStatus, CheckoutInput, PaymentGateway } from "../model";

type Data = Record<string, unknown>;
function object(value: unknown): Data { return value && typeof value === "object" && !Array.isArray(value) ? value as Data : {}; }
function text(value: unknown): string | undefined { return typeof value === "string" && value.length > 0 ? value : undefined; }
function seconds(value: unknown): number {
  const date = typeof value === "string" ? Date.parse(value) : NaN;
  if (!Number.isFinite(date)) throw new Error("Invalid billing timestamp");
  return Math.floor(date / 1000);
}
function equalHex(value: unknown, expected: Buffer): boolean {
  return typeof value === "string" && /^[a-f0-9]{64}$/i.test(value) && timingSafeEqual(Buffer.from(value,"hex"),expected);
}
const statuses = new Set<string>(["active","past_due","canceled","trialing","paused"]);
const subscriptionEvents = new Set(["subscription.created","subscription.activated","subscription.updated","subscription.canceled","subscription.past_due"]);
export class PaddleGateway implements PaymentGateway {
  private origin: string;
  constructor(private key: string, private secret: string, private mode: BillingMode,
    private plans: Record<string,string>, private checkoutUrl: string,
    private request: typeof fetch = fetch, private now: () => number = Date.now) {
    this.origin = mode === "test" ? "https://sandbox-api.paddle.com" : "https://api.paddle.com";
    if (!key.startsWith(mode === "test" ? "pdl_sdbx_apikey_" : "pdl_live_apikey_")) throw new Error("Billing key mode mismatch");
  }
  private async api(path: string, data?: unknown): Promise<Data> {
    const response = await this.request(`${this.origin}${path}`,{
      method:data === undefined ? "GET" : "POST",
      headers:{Authorization:`Bearer ${this.key}`,"Content-Type":"application/json","Paddle-Version":"1"},
      ...(data === undefined ? {} : {body:JSON.stringify(data)}), signal:AbortSignal.timeout(15000),
    });
    if (!response.ok) throw new Error("Paddle request failed");
    return object(object(await response.json()).data);
  }
  private binding(uid: string): Buffer {
    return createHmac("sha256",this.secret).update(`checkout:${this.mode}:${uid}`).digest();
  }
  async createCheckout(input: CheckoutInput): Promise<string> {
    if (!Object.values(this.plans).includes(input.price)) throw new Error("Unknown billing price");
    const transaction = await this.api("/transactions",{
      items:[{price_id:input.price,quantity:1}],collection_mode:"automatic",
      custom_data:{uid:input.uid,billing_binding:this.binding(input.uid).toString("hex")},
      checkout:{url:this.checkoutUrl},
      ...(input.customer ? {customer_id:input.customer} : {}),
    });
    if (typeof transaction.id !== "string" || !/^txn_[a-z0-9]{26}$/.test(transaction.id)) throw new Error("Checkout transaction missing");
    return `${this.checkoutUrl}?_ptxn=${transaction.id}`;
  }
  async createPortal(customer: string, _returnUrl: string): Promise<string> {
    if (!/^ctm_[a-z0-9]{26}$/.test(customer)) throw new Error("Invalid billing customer");
    const session = await this.api(`/customers/${customer}/portal-sessions`,{});
    const url = text(object(object(session.urls).general).overview);
    if (!url || new URL(url).protocol !== "https:") throw new Error("Portal URL missing");
    return url;
  }
  private subscriptionChange(data: Data): NonNullable<BillingEvent["change"]> {
    const items = Array.isArray(data.items) ? data.items.map(object) : [];
    const price = object(items[0]?.price).id;
    const plan = Object.entries(this.plans).find(([,id]) => id === price)?.[0];
    if (!plan || items.length !== 1 || !statuses.has(String(data.status))) throw new Error("Unsupported subscription");
    const period = object(data.current_billing_period);
    return {plan,status:data.status as BillingStatus,
      current_period_end:period.ends_at ? seconds(period.ends_at) : seconds(data.canceled_at),
      cancel_at_period_end:object(data.scheduled_change).action === "cancel"};
  }
  async verifyAndParseWebhook(body: Buffer, signature: string): Promise<BillingEvent> {
    const parts = signature.split(";").map(part => part.trim().split("="));
    const times = parts.filter(([key]) => key === "ts");
    const timestamp = times[0]?.[1];
    if (times.length !== 1 || !timestamp || !/^\d+$/.test(timestamp) || Math.abs(this.now()/1000 - Number(timestamp)) > 5) throw new Error("Invalid webhook timestamp");
    const expected = createHmac("sha256",this.secret).update(`${timestamp}:`).update(body).digest();
    if (!parts.some(([key,value]) => key === "h1" && equalHex(value,expected))) throw new Error("Invalid webhook signature");
    const event = object(JSON.parse(body.toString("utf8")));
    if (!text(event.event_id)?.match(/^evt_[a-z0-9]{26}$/) || !text(event.event_type)) throw new Error("Invalid billing event");
    const data = object(event.data);
    const customer = text(data.customer_id) ?? null;
    let change: BillingEvent["change"] = null;
    if (subscriptionEvents.has(String(event.event_type))) change = this.subscriptionChange(data);
    else if (["transaction.completed","transaction.payment_failed"].includes(String(event.event_type)) && text(data.subscription_id)) {
      const subscription = await this.api(`/subscriptions/${encodeURIComponent(String(data.subscription_id))}`);
      if (subscription.customer_id !== customer) throw new Error("Billing customer mismatch");
      change = this.subscriptionChange(subscription);
      change.status = event.event_type === "transaction.completed" ? "active" : "past_due";
      if (event.event_type === "transaction.completed" && object(data.billing_period).ends_at) change.current_period_end = seconds(object(data.billing_period).ends_at);
    } else if (["adjustment.created","adjustment.updated"].includes(String(event.event_type)) && data.status === "approved"
      && ["refund","chargeback","chargeback_warning"].includes(String(data.action))) change = {status:"revoked"};
    const custom = object(data.custom_data);
    let uid: string | undefined;
    if (change && custom.uid !== undefined) {
      uid = text(custom.uid);
      if (!uid || uid.includes("/") || !equalHex(custom.billing_binding,this.binding(uid))) throw new Error("Invalid checkout identity");
    }
    if (change && (!customer || !/^ctm_[a-z0-9]{26}$/.test(customer))) throw new Error("Missing billing customer");
    return {id:String(event.event_id),mode:this.mode,created:seconds(event.occurred_at),customer,change,...(uid ? {uid} : {})};
  }
}

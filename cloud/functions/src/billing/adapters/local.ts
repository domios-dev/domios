import { createHmac, timingSafeEqual } from "node:crypto";
import type { BillingEvent, CheckoutInput, PaymentGateway } from "../model";
export class LocalGateway implements PaymentGateway {
  constructor(private secret: string, private origin = "http://127.0.0.1:15100") {}
  setOrigin(origin: string): void { this.origin = origin; }
  async createCustomer(uid: string): Promise<string> { return `local_${Buffer.from(uid).toString("base64url")}`; }
  async createCheckout(input: CheckoutInput): Promise<string> { return `${this.origin}/billing/checkout?customer=${encodeURIComponent(input.customer)}&plan=${encodeURIComponent(input.price)}`; }
  async createPortal(customer: string): Promise<string> { return `${this.origin}/billing/portal?customer=${encodeURIComponent(customer)}`; }
  sign(body: Buffer): string { return createHmac("sha256",this.secret).update(body).digest("hex"); }
  async verifyAndParseWebhook(body: Buffer, signature: string): Promise<BillingEvent> {
    const expected = Buffer.from(this.sign(body));
    const supplied = Buffer.from(signature);
    if (expected.length !== supplied.length || !timingSafeEqual(expected,supplied)) throw new Error("Invalid signature");
    const event = JSON.parse(body.toString()) as BillingEvent;
    if (event.mode !== "test" || !event.id || !Number.isFinite(event.created)) throw new Error("Invalid local event");
    return event;
  }
}

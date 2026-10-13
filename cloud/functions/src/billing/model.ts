export type BillingMode = "test" | "live";
export type BillingStatus = "active" | "past_due" | "canceled" | "revoked" | "incomplete" | "trialing" | "unpaid" | "paused";
export interface Entitlement {
  plan: string;
  status: BillingStatus;
  current_period_end: number;
  cancel_at_period_end: boolean;
  source_event_id: string;
  updated_at: number;
}
export interface BillingEvent {
  id: string;
  mode: BillingMode;
  created: number;
  customer: string | null;
  uid?: string;
  change: Partial<Pick<Entitlement, "plan" | "status" | "current_period_end" | "cancel_at_period_end">> | null;
}
export interface CheckoutInput { uid: string; customer: string; price: string; successUrl: string; cancelUrl: string }
export interface PaymentGateway {
  createCustomer?(uid: string, idempotencyKey: string): Promise<string>;
  createCheckout(input: CheckoutInput): Promise<string>;
  createPortal(customer: string, returnUrl: string): Promise<string>;
  verifyAndParseWebhook(body: Buffer, signature: string): Promise<BillingEvent>;
}
export function collectionName(mode: BillingMode, name: string): string { return `${mode === "test" ? "test_" : ""}${name}`; }
export function reduceEntitlement(previous: Entitlement | undefined, event: BillingEvent): Entitlement | undefined {
  if (!event.change || (previous && (event.created < previous.updated_at || previous.source_event_id === event.id))) return previous;
  if (!previous && (!event.change.plan || !event.change.status) && event.change.status !== "revoked") return undefined;
  const status = previous?.status === "revoked" ? "revoked" : event.change.status ?? previous?.status ?? "incomplete";
  return {
    plan: event.change.plan ?? previous?.plan ?? "none",
    status,
    current_period_end: event.change.current_period_end ?? previous?.current_period_end ?? 0,
    cancel_at_period_end: event.change.cancel_at_period_end ?? previous?.cancel_at_period_end ?? false,
    source_event_id: event.id,
    updated_at: event.created,
  };
}

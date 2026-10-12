import Stripe from "stripe";
import { StripeGateway } from "./stripe";
import { LocalGateway } from "./local";
import { FakeGateway } from "./fake";
import type { BillingMode, PaymentGateway } from "../model";
export function paymentGateway(provider: string, config: {key?: string; secret: string; mode: BillingMode; plans: Record<string,string>; origin?: string}): PaymentGateway {
  switch (provider) {
    case "stripe":
      if (!config.key?.startsWith(config.mode === "test" ? "sk_test_" : "sk_live_")) throw new Error("Billing key mode mismatch");
      return new StripeGateway(new Stripe(config.key),config.secret,config.mode,config.plans);
    case "fake":
    case "local":
      if (config.mode !== "test") throw new Error("Local billing only supports test mode");
      return provider === "fake" ? new FakeGateway(config.secret,config.origin) : new LocalGateway(config.secret,config.origin);
    default: throw new Error("Unknown payment gateway");
  }
}

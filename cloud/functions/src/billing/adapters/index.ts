import { PaddleGateway } from "./paddle";
import { LocalGateway } from "./local";
import { FakeGateway } from "./fake";
import type { BillingMode, PaymentGateway } from "../model";
export function paymentGateway(provider: string, config: {key?: string; secret: string; mode: BillingMode; plans: Record<string,string>; origin?: string}): PaymentGateway {
  switch (provider) {
    case "paddle":
      if (!config.key || !config.secret || !config.origin) throw new Error("Paddle configuration missing");
      return new PaddleGateway(config.key,config.secret,config.mode,config.plans,config.origin);
    case "fake":
    case "local":
      if (config.mode !== "test") throw new Error("Local billing only supports test mode");
      return provider === "fake" ? new FakeGateway(config.secret,config.origin) : new LocalGateway(config.secret,config.origin);
    default: throw new Error("Unknown payment gateway");
  }
}

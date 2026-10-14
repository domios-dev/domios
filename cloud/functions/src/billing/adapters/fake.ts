import { LocalGateway } from "./local";
import type { CheckoutInput } from "../model";
export class FakeGateway extends LocalGateway {
  customers: string[] = [];
  checkouts: CheckoutInput[] = [];
  override async createCustomer(uid: string): Promise<string> { this.customers.push(uid); return super.createCustomer(uid); }
  override async createCheckout(input: CheckoutInput): Promise<string> { this.checkouts.push(input); return super.createCheckout(input); }
}

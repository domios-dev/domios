import { BillingError } from "./service";
import type { Entitlement } from "./model";
export type ClientEntitlements = Pick<Entitlement, "plan" | "status" | "current_period_end" | "cancel_at_period_end">;
export const freeEntitlements: ClientEntitlements = {plan:"free",status:"active",current_period_end:0,cancel_at_period_end:false};
export async function getOwnEntitlements(uid: string | undefined, data: unknown, read: (uid: string) => Promise<Entitlement | undefined>): Promise<ClientEntitlements> {
  if (!uid) throw new BillingError("unauthenticated","Authentication required");
  if (!data || typeof data !== "object" || Array.isArray(data)
    || Object.keys(data).some(key => key !== "uid") || ("uid" in data && data.uid !== uid)) {
    throw new BillingError("invalid-argument","Only your own entitlements may be requested");
  }
  const value = await read(uid);
  if (!value) return {...freeEntitlements};
  return {plan:value.plan,status:value.status,current_period_end:value.current_period_end,cancel_at_period_end:value.cancel_at_period_end};
}

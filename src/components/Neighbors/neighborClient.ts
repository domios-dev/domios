import { localFetch } from "../../lib/webApi";
import { parseNeighbors, type NeighborCommand, type NeighborList } from "./neighborModel";
import { parseReferralClaim, type ReferralClaim } from "./referralTrial";

export interface NeighborClient {
  list(signal?: AbortSignal): Promise<NeighborList>;
  claimReferralTrial(signal?: AbortSignal): Promise<ReferralClaim>;
  apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<void>;
}

async function call(command: NeighborCommand | { operation: "claim_referral_trial" }, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/neighbors", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(command), signal,
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

export const neighborClient: NeighborClient = {
  claimReferralTrial: async signal => parseReferralClaim(await call({ operation: "claim_referral_trial" }, signal)),
  list: async signal => parseNeighbors(await call({ operation: "list" }, signal)),
  apply: async command => { await call(command); },
};

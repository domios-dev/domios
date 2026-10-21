import assert from "node:assert/strict";
import {creditExpiryHint, hasExpiringCredits} from "./creditExpiry";
import type {UsageDetails} from "../../types/usage";
const now = Date.UTC(2026, 9, 21);
const hour = 3_600_000;
assert.equal(creditExpiryHint(now + 72 * hour, now), "expires in 3 days");
assert.equal(creditExpiryHint(now + 72 * hour + 1, now), null);
assert.equal(creditExpiryHint(now + 23 * hour, now), "expires in 23 hours");
assert.equal(creditExpiryHint(now + 1, now), "expires in 1 hour");
for (const expiry of [null, undefined, NaN, now, now - 1]) assert.equal(creditExpiryHint(expiry, now), null);
const reset = {id:"one",status:"available" as const,expiresAt:now+hour};
assert.equal(hasExpiringCredits({availableCount:1,nextExpiry:null,credits:[reset]}, null, now), true);
assert.equal(hasExpiringCredits({availableCount:0,nextExpiry:now+hour,credits:[reset]}, null, now), false);
for (const status of ["redeemed", "redeeming", "unknown"] as const) {
  assert.equal(hasExpiringCredits({availableCount:1,nextExpiry:now+hour,credits:[{...reset,status}]}, null, now), false);
}
assert.equal(hasExpiringCredits({availableCount:1,nextExpiry:now+hour,credits:null}, null, now), true);
assert.equal(hasExpiringCredits({availableCount:2,nextExpiry:now+hour,credits:[{...reset,expiresAt:null}]}, null, now), true);
const details: UsageDetails = {limits:[],extra_usage:null,dollar_credits:[{remaining_dollars:250,resets_at:now+hour}]};
assert.equal(hasExpiringCredits(null, details, now), true);
assert.equal(hasExpiringCredits(null, {...details,dollar_credits:[{remaining_dollars:0,resets_at:now+hour}]}, now), false);
assert.equal(hasExpiringCredits(null, null, now), false);
console.log("creditExpiry: passed");

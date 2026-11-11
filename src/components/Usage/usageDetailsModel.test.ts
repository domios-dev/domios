import assert from "node:assert/strict";
import {dollarCredits, extraUsage, usageField, usageLimits} from "./usageDetailsModel";

for (const details of [null, undefined, [], 3, "opaque", {balance:{available:true,amounts:[]}}, {quota:{title:"quota",limits:[]}}, {custom:{key:"kept"}}]) {
  assert.deepEqual(usageLimits(details), []);
  assert.deepEqual(dollarCredits(details), []);
  assert.equal(extraUsage(usageField(details, "extra_usage")), null);
}
assert.deepEqual(usageLimits({limits:[null, {}, {kind:"session",label:"Session",percent:25,resets_at:null,is_active:true}]}),
  [{kind:"session",label:"Session",percent:25,resets_at:null,is_active:true,critical:false}]);
assert.deepEqual(dollarCredits({dollar_credits:[null,{remaining_dollars:4,resets_at:null}]}), [{remaining_dollars:4,resets_at:null}]);
console.log("usageDetailsModel: passed");

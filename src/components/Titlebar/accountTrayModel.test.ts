import assert from "node:assert/strict";
import { accountTrayModel } from "./accountTrayModel";
const out = accountTrayModel({kind:"signed_out"}, null);
assert.equal(out.name, "Account");
assert.equal(out.disabled, false);
assert.equal(accountTrayModel({kind:"pending"}, null).disabled, true);
assert.equal(accountTrayModel({kind:"error",message:"Request failed"}, null).error, "Request failed");
const identity = {uid:"user",display_name:"Ada",email:"ada@example.test",provider:"fake"};
const signedIn = accountTrayModel({kind:"signed_in",identity}, {authority:"billing",fullAccess:false,plan:"pro",status:"active",stale:false,periodEnd:null,cancelAtPeriodEnd:false});
assert.equal(signedIn.initial, "A");
assert.equal(signedIn.identity, "Ada");
assert.equal(signedIn.plan, "pro (active)");
assert.equal(accountTrayModel({kind:"signed_in",identity:{...identity,display_name:null}}, null).identity, identity.email);

for (const authority of ["admin", "billing", "free"] as const) {
  for (const stale of [false, true]) {
    const result = accountTrayModel({kind:"signed_in",identity}, {authority,fullAccess:authority === "admin",plan:"free",status:"active",stale,periodEnd:null,cancelAtPeriodEnd:false});
    assert.equal(result.plan, `${authority === "admin" ? "Admin · " : ""}free (active)${stale ? " · stale" : ""}`);
  }
}
console.log("14 model assertions passed");

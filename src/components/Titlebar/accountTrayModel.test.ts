import assert from "node:assert/strict";
import { accountAvatarUrl, accountTrayModel, planBadge } from "./accountTrayModel";
import { accountInitials } from "../Settings/accountModel";
const out = accountTrayModel({kind:"signed_out"}, null);
assert.equal(out.name, "Account");
assert.equal(out.disabled, false);
assert.equal(accountTrayModel({kind:"pending"}, null).disabled, true);
assert.equal(accountTrayModel({kind:"error",message:"Request failed"}, null).error, "Request failed");
const identity = {uid:"user",display_name:"Ada",email:"ada@example.test",provider:"fake"};
const signedIn = accountTrayModel({kind:"signed_in",identity}, {authority:"billing",fullAccess:false,plan:"pro",status:"active",stale:false,periodEnd:null,cancelAtPeriodEnd:false,limits:null});
assert.equal(signedIn.initial, accountInitials("Ada"));
assert.equal(signedIn.identity, "Ada");
assert.equal(signedIn.plan, "Pro · active");
assert.equal(accountTrayModel({kind:"signed_in",identity:{...identity,display_name:null}}, null).identity, identity.email);
assert.deepEqual(signedIn.badge, { tier: "pro", name: "Pro", status: "active", tone: "active", admin: false, stale: false });
const plan = (status: string) => ({authority:"billing" as const,fullAccess:false,plan:"sponsor",status,stale:false,periodEnd:null,cancelAtPeriodEnd:false,limits:null});
assert.deepEqual([planBadge(plan("past_due"))?.status, planBadge(plan("past_due"))?.tone], ["past due", "attention"]);
assert.equal(planBadge(plan("canceled"))?.tone, "inactive");
assert.equal(planBadge(plan("trialing"))?.name, "Sponsor");
assert.equal(planBadge(null), null);

for (const authority of ["admin", "billing", "free"] as const) {
  for (const stale of [false, true]) {
    const result = accountTrayModel({kind:"signed_in",identity}, {authority,fullAccess:authority === "admin",plan:"free",status:"active",stale,periodEnd:null,cancelAtPeriodEnd:false,limits:null});
    assert.equal(result.plan, `${authority === "admin" ? "Admin · " : ""}Free · active${stale ? " · stale" : ""}`);
  }
}
const photo = "https://lh3.googleusercontent.com/a/fixture=s96-c";
assert.equal(accountAvatarUrl(photo), photo);
assert.equal(accountTrayModel({kind:"signed_in",identity:{...identity,photo_url:photo}}, null).avatarUrl, photo);
assert.equal(signedIn.avatarUrl, null);
for (const rejected of [null, undefined, "", "http://lh3.googleusercontent.com/a", "javascript:alert(1)", "data:image/png;base64,AAAA",
  "https://user:pass@lh3.googleusercontent.com/a", "not a url", `https://lh3.googleusercontent.com/${"a".repeat(2048)}`]) {
  assert.equal(accountAvatarUrl(rejected), null);
}
assert.equal(accountInitials("Domios User"), "DU");
assert.equal(accountInitials("domios.user@example.test"), "DU");
console.log("28 model assertions passed");

import assert from "node:assert/strict";
import { accountPlanModel, manageActionForPlan, planFailure } from "./accountPlanModel";
const free = accountPlanModel({entitlements:{plan:"free",status:"active",current_period_end:0,cancel_at_period_end:false},stale:false,access:{authority:"free",full_access:false}});
assert.equal(free.plan,"free");
assert.equal(free.periodEnd,null);
const paid = accountPlanModel({entitlements:{plan:"pro",status:"past_due",current_period_end:2000000000,cancel_at_period_end:true},stale:true,access:{authority:"billing",full_access:false}});
assert.equal(paid.status,"past_due");
assert.equal(paid.cancelAtPeriodEnd,true);
assert.equal(paid.periodEnd,2000000000);
assert.equal(paid.stale,true);
assert.equal(manageActionForPlan(paid),"portal");
assert.deepEqual(planFailure({...paid,stale:false}),paid);
assert.equal(planFailure(null),null);

const admin = accountPlanModel({entitlements:{plan:"free",status:"active",current_period_end:0,cancel_at_period_end:false},stale:false,access:{authority:"admin",full_access:true}});
assert.equal(admin.authority,"admin");
assert.equal(admin.fullAccess,true);
assert.equal(free.authority,"free");
assert.equal(paid.authority,"billing");
assert.equal(manageActionForPlan(free),"plans");
assert.equal(manageActionForPlan(admin),"plans");
assert.equal(manageActionForPlan(null),"plans");
assert.equal(manageActionForPlan({...paid,authority:"free"}),"portal");
assert.equal(manageActionForPlan({...admin,plan:"sponsor"}),"portal");
const future = accountPlanModel({
  signed_tier: "future-tier",
  entitlements: { plan: "pro", status: "active", current_period_end: 2000000000, cancel_at_period_end: false },
  access: { authority: "billing", full_access: false }, stale: false,
  limits: { enforcing: true, workspaces: 19, teams: 8, "agent.tabs": 64 },
});
assert.equal(future.plan, "future-tier");
assert.equal(future.limits?.max.workspaces, 19);
assert.equal(future.limits?.max.teams, 8);
assert.equal(future.limits?.max["agent.tabs"], 64);
console.log("22 model assertions passed");

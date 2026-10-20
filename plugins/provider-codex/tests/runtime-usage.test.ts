import assert from "node:assert/strict";
import fs from "node:fs";
import {rateLimitEvents} from "../src/runtimeUsage";

const fixture = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-rate-limits-credits.json",import.meta.url),"utf8"));
const balance = rateLimitEvents(fixture.balance.rateLimits);
const unlimited = rateLimitEvents(fixture.unlimited.rateLimits);
const noCredits = rateLimitEvents(fixture.noCredits.rateLimits);
const absent = rateLimitEvents(fixture.absent.rateLimits);

assert.deepEqual(balance[0],{
  kind:"rateLimits",
  rateLimits:[{window:"primary",usedPercent:25,resetsAt:1792472484000,windowMinutes:300}],
  accountCredits:{kind:"balance",balance:"123.4500"},
});
assert.deepEqual(unlimited[0],{
  kind:"rateLimits",
  rateLimits:[{window:"primary",usedPercent:25,resetsAt:1792472484000,windowMinutes:300}],
  accountCredits:{kind:"unlimited"},
});
assert.deepEqual(noCredits[0],{
  kind:"rateLimits",
  rateLimits:[{window:"primary",usedPercent:25,resetsAt:1792472484000,windowMinutes:300}],
  accountCredits:{kind:"balance",balance:"0"},
});
assert.deepEqual(absent[0],{kind:"rateLimits",rateLimits:noCredits[0].rateLimits});
assert.deepEqual(rateLimitEvents({limitId:"codex",credits:{hasCredits:true,unlimited:false,balance:"8.25"}})[0],{
  kind:"rateLimits",
  rateLimits:[],
  accountCredits:{kind:"balance",balance:"8.25"},
});
assert.deepEqual(rateLimitEvents({credits:{hasCredits:false,unlimited:false,balance:"0.0000"}}),[
  {kind:"rateLimits",rateLimits:[],accountCredits:{kind:"balance",balance:"0.0000"}},
]);
for (const balance of [null, "", "unknown", "-1", "NaN"]) {
  assert.deepEqual(rateLimitEvents({credits:{hasCredits:false,unlimited:false,balance}}),[]);
}
console.log("PASS Codex runtime account credits: balance, unlimited, unavailable and absent");

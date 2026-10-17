import assert from "node:assert/strict";
import {resetCredits, resetOutcome} from "../src/resetCredits";

let count = 0;
function check(name: string, run: () => void) {
  run();
  count++;
  console.log(`  ✓ ${name}`);
}

check("missing summaries remain unsupported", () => {
  for (const raw of [null, undefined, {}, {availableCount: -1}, {availableCount: 1.5}]) {
    assert.equal(resetCredits(raw), undefined);
  }
});

check("count-only summaries preserve unknown details", () => {
  assert.deepEqual(resetCredits({availableCount: 3, credits: null}), {
    availableCount: 3, nextExpiry: null, credits: null,
  });
  assert.deepEqual(resetCredits({availableCount: 0}), {
    availableCount: 0, nextExpiry: null, credits: null,
  });
});

check("details select the earliest available expiry in milliseconds", () => {
  const result = resetCredits({availableCount: 2, credits: [
    {id: "used", status: "redeemed", expiresAt: 1},
    {id: "later", status: "available", expiresAt: 300, title: "Gift", description: "Reset limits"},
    {id: "next", status: "available", expiresAt: 200},
  ]});
  assert.equal(result?.nextExpiry, 200_000);
  assert.deepEqual(result?.credits?.[1], {
    id: "later", status: "available", expiresAt: 300_000, title: "Gift", description: "Reset limits",
  });
});

check("non-expiring credits have no next expiry", () => {
  assert.deepEqual(resetCredits({availableCount: 1, credits: [
    {id: "forever", status: "available", expiresAt: null},
  ]}), {availableCount: 1, nextExpiry: null, credits: [{id: "forever", status: "available", expiresAt: null}]});
});

check("capped details never replace the authoritative count", () => {
  const result = resetCredits({availableCount: 10, credits: [{id: "one", status: "available", expiresAt: 50}]});
  assert.equal(result?.availableCount, 10);
  assert.equal(result?.credits?.length, 1);
  assert.equal(result?.nextExpiry, 50_000);
});

check("unknown statuses cannot contribute an expiry", () => {
  const result = resetCredits({availableCount: 1, credits: [{id: "future", status: "future", expiresAt: 1}]});
  assert.equal(result?.nextExpiry, null);
  assert.equal(result?.credits?.[0].status, "unknown");
});

check("all consume outcomes are recognized without issuing a request", () => {
  for (const outcome of ["reset", "nothingToReset", "noCredit", "alreadyRedeemed"]) {
    assert.equal(resetOutcome({outcome}), outcome);
  }
  for (const raw of [null, {}, {outcome: "future"}]) assert.equal(resetOutcome(raw), undefined);
});



check("out-of-range expiry stays unknown instead of breaking date rendering", () => {
  const result = resetCredits({availableCount:1,credits:[{id:"invalid-date",status:"available",expiresAt:1e99}]});
  assert.equal(result?.nextExpiry,null);
  assert.equal(result?.credits?.[0].expiresAt,null);
});

console.log(`${count} reset credit mapping checks passed`);

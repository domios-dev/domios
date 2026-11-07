import assert from "node:assert/strict";
import type { UsageSnapshot } from "../../types/usage";
import { balanceLabel, balanceNeedsRead, balanceReading } from "./balanceModel";

const row = (details: unknown, extra: Partial<UsageSnapshot> = {}) => ({provider: "fixture", account_id: "profile:fixture", captured_at: 1_000, parsed_ok: true, usage_details: details, ...extra}) as UsageSnapshot;
const reading = balanceReading(row({balance: {available: true, amounts: [{currency: "CNY", amount: "110.00"}, {currency: "USD", amount: "3.5"}]}}));
assert.deepEqual(reading, {available: true, amounts: [{currency: "CNY", amount: "110.00"}, {currency: "USD", amount: "3.5"}]});
assert.equal(balanceLabel(reading!), "110 CNY · 3.5 USD");
assert.equal(balanceLabel({available: false, amounts: []}), "no balance");
for (const details of [null, {}, {balance: {amounts: []}}, {balance: {available: true, amounts: [{currency: "CNY"}]}}, {balance: {available: "yes", amounts: []}}]) {
  assert.equal(balanceReading(row(details)), null, JSON.stringify(details));
}
assert.equal(balanceReading(row({balance: {available: true, amounts: []}}, {parsed_ok: false})), null, "failure rows carry no balance");
assert.equal(balanceNeedsRead(null, 0), true);
assert.equal(balanceNeedsRead(row({}), 1_000 + 60_000), false);
assert.equal(balanceNeedsRead(row({}), 1_000 + 21 * 60_000), true);
assert.equal(balanceNeedsRead(row({}, {parsed_ok: false}), 1_000), true);
console.log("balanceModel: validated balance shape, label and read-on-demand staleness");

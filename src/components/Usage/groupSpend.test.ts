import assert from "node:assert/strict";
import type { ModelGroupUsage } from "../../lib/types";
import { groupSpendSnapshot, hasGroupSpend } from "./groupSpend";

function usage(overrides: Partial<ModelGroupUsage> = {}): ModelGroupUsage {
  return {
    group: "deepseek",
    messages: 3,
    input_tokens: 100,
    output_tokens: 40,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cost_micros: 339_000,
    ...overrides,
  };
}

assert.equal(hasGroupSpend(usage({ spend_cents: 34 })), true, "reported spend is shown");
assert.equal(hasGroupSpend(usage()), false, "a group without spend renders nothing");
assert.equal(hasGroupSpend(null), false, "no usage renders nothing");

const live = groupSpendSnapshot("opencode", usage({ spend_cents: 34, spend_remaining_cents: 62 }), 1234);
assert.equal(live.provider, "opencode");
assert.equal(live.account_id, "deepseek", "the group is the account");
assert.equal(live.captured_at, 1234);
assert.equal(live.parsed_ok, true);
assert.equal(live.spend_cents, 34);
assert.equal(live.spend_remaining_cents, 62);
assert.equal(live.spend_limit_cents, null, "an unreadable limit stays null, never fabricated");
assert.equal(live.session_pct, null);
assert.equal(live.weekly_pct, null);

const spendOnly = groupSpendSnapshot("opencode", usage({ spend_cents: 34 }), 0);
assert.equal(spendOnly.spend_cents, 34);
assert.equal(spendOnly.spend_remaining_cents, null, "no live balance ⇒ null remaining");
assert.equal(spendOnly.spend_limit_cents, null);

console.log("usage group spend: all passed");

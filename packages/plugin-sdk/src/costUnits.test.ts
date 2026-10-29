import assert from "node:assert/strict";
import { test } from "node:test";
import fixture from "./fixtures/cost-units.json";
import { formatCostAmount } from "./costUnits";

test("cost unit host/SDK shared exact decimal fixtures", () => {
  for (const {amount,unit,expected} of fixture.cases) assert.equal(formatCostAmount(amount,unit,fixture.units),expected);
  assert.throws(() => formatCostAmount(Number.MAX_SAFE_INTEGER+1,"credits",fixture.units));
  assert.throws(() => formatCostAmount(1,"credits",{credits:{one:"credit",other:"credits",currency:{code:"USD",scale:16}}}));
  assert.throws(() => formatCostAmount(1,"credits",{credits:{one:"credit\n",other:"credits"}}));
});

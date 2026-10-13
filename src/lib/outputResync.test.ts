import assert from "node:assert/strict";
import { finish, it } from "../test/standaloneTest";
import { planOutputResync } from "./outputResync";

it("adopts a stale cursor once and suppresses the same gap on resync", () => {
  const first = planOutputResync(12, 42295516);
  assert.deepEqual(first, { offset: 42295516, reportGap: true });
  assert.deepEqual(planOutputResync(first.offset, 42295516), { offset: 42295516, reportGap: false });
});

it("normal resumes and older gap notices never regress the cursor or report a gap", () => {
  assert.deepEqual(planOutputResync(42, 42), { offset: 42, reportGap: false });
  assert.deepEqual(planOutputResync(42, 12), { offset: 42, reportGap: false });
});

it("initial eviction and a later distinct gap each advance the cursor", () => {
  assert.deepEqual(planOutputResync(undefined, 0), { offset: 0, reportGap: false });
  assert.deepEqual(planOutputResync(undefined, 100), { offset: 100, reportGap: true });
  assert.deepEqual(planOutputResync(100, 200), { offset: 200, reportGap: true });
});

finish();

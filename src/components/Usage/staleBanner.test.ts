import assert from "node:assert/strict";
import test from "node:test";
import {
  staleBannerCopy,
  STALE_CAUSE_CAPABILITY_DENIED,
  STALE_CAUSE_PROBE_FAILED,
  STALE_CAUSE_UNKNOWN,
} from "./staleBanner";
import { CAPABILITY_DENIED_KIND } from "./UsageTable";
import type { UsageSnapshot } from "../../types/usage";

const row = (failure_reason: string | null): UsageSnapshot =>
  ({ failure_reason, parsed_ok: true, provider: "codex" } as unknown as UsageSnapshot);

// A provider that loses a capability used to show a frozen gauge and a banner
// that said nothing but "may be outdated". The cause code is what makes that
test("a capability-denied probe gives the stale banner its own cause code", () => {
  const copy = staleBannerCopy(row(`${CAPABILITY_DENIED_KIND}: plugin denied readFile`));
  assert.equal(copy.cause, STALE_CAUSE_CAPABILITY_DENIED);
  assert.notEqual(copy.detail, null, "the user is told what to do about it");
});

test("an ordinary probe failure is a distinct cause from a confined plugin", () => {
  const failed = staleBannerCopy(row("FETCH_ERROR: request timed out"));
  assert.equal(failed.cause, STALE_CAUSE_PROBE_FAILED);
  assert.notEqual(
    failed.cause,
    staleBannerCopy(row(`${CAPABILITY_DENIED_KIND}: denied`)).cause,
    "an upstream failure must not read as confinement",
  );
});

test("staleness with no recorded failure stays the plain outdated notice", () => {
  const copy = staleBannerCopy(row(null));
  assert.equal(copy.cause, STALE_CAUSE_UNKNOWN);
  assert.equal(copy.detail, null);
});

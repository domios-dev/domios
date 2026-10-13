import assert from "node:assert/strict";
import { test } from "node:test";
import { entitlementDiagnostics } from "../src/domain/diagnostics.ts";

const entitlement = {
  plan: "pro", status: "active", current_period_end: 0,
  cancel_at_period_end: false, source_event_id: "evt_fixture", updated_at: 1,
};

test("missing entitlement explains the billing service's free fallback", () => {
  assert.deepEqual(entitlementDiagnostics(null), {
    plan: "free", status: "active", provenance: "Default free entitlement", eventId: null,
  });
});

test("webhook provenance retains the exact source event", () => {
  assert.deepEqual(entitlementDiagnostics(entitlement), {
    plan: "pro", status: "active", provenance: "Paddle webhook", eventId: "evt_fixture",
  });
});

test("admin provenance takes precedence over the event identifier", () => {
  assert.deepEqual(entitlementDiagnostics({ ...entitlement, source: "admin", status: "revoked" }), {
    plan: "pro", status: "revoked", provenance: "Admin grant or revocation", eventId: "evt_fixture",
  });
});

test("unknown or missing provenance is not presented as a webhook", () => {
  assert.equal(entitlementDiagnostics({ ...entitlement, source_event_id: "unclassified" }).provenance, "Unknown source");
  assert.deepEqual(entitlementDiagnostics({ ...entitlement, source_event_id: "" }), {
    plan: "pro", status: "active", provenance: "Unknown source", eventId: null,
  });
});

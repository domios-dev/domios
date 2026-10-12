import type { Entitlement } from "./model";

export interface EntitlementDiagnostics {
  plan: string;
  status: string;
  provenance: string;
  eventId: string | null;
}

export function entitlementDiagnostics(entitlement: Entitlement | null): EntitlementDiagnostics {
  if (!entitlement) return { plan: "free", status: "active", provenance: "Default free entitlement", eventId: null };
  const eventId = entitlement.source_event_id || null;
  const provenance = entitlement.source === "admin" ? "Admin grant or revocation"
    : eventId?.startsWith("evt_") ? "Stripe webhook"
    : "Unknown source";
  return { plan: entitlement.plan, status: entitlement.status, provenance, eventId };
}

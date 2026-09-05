import type { UsageSnapshot } from "../../types/usage";
import { CAPABILITY_DENIED_KIND, usageFailureKind, usageFailureLabel } from "./UsageTable";

/// Cause codes for the stale banner. `data-usage-stale-cause` carries the code,
/// so the reason a gauge stopped moving is assertable without pinning wording.
export const STALE_CAUSE_UNKNOWN = "unknown";
export const STALE_CAUSE_PROBE_FAILED = "probe-failed";
export const STALE_CAUSE_CAPABILITY_DENIED = "capability-denied";

export interface StaleBannerCopy {
  cause: string;
  headline: string;
  detail: string | null;
}

/// A number that quietly stops moving is the worst failure this panel has: the
/// last probe's failure rides on the row, so staleness always names its cause.
export function staleBannerCopy(snapshot: UsageSnapshot): StaleBannerCopy {
  const kind = usageFailureKind(snapshot);
  if (!kind) {
    return { cause: STALE_CAUSE_UNKNOWN, headline: "Data may be outdated", detail: null };
  }
  if (kind === CAPABILITY_DENIED_KIND) {
    return {
      cause: STALE_CAUSE_CAPABILITY_DENIED,
      headline: "Not updating",
      detail:
        `The ${snapshot.provider} plugin is unverified, so it runs confined and was denied access it needs. ` +
        "Reinstall it from the official channel to restore this panel.",
    };
  }
  return {
    cause: STALE_CAUSE_PROBE_FAILED,
    headline: "Not updating",
    detail: `The last usage probe ${usageFailureLabel(snapshot)}.`,
  };
}

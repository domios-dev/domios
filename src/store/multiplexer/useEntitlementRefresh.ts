import { useEffect } from "react";
import { freeLimitGate } from "../../components/billing/freeLimitGate";
import { parseFreeLimits } from "../../components/billing/freeLimitModel";
import { listen } from "../../lib/bridge";

/** A plan change moves the daemon's lock marks and limits; re-read the snapshot that carries them. */
export function useEntitlementRefresh(refresh: (generation?: number, force?: boolean) => Promise<boolean>): void {
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<{ limits?: unknown } | null>("entitlement-changed", (event) => {
      const limits = parseFreeLimits(event.payload?.limits);
      if (limits) freeLimitGate.setLimits(limits);
      void refresh(undefined, true);
    }).then((fn) => {
      if (active) unlisten = fn;
      else fn();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [refresh]);
}

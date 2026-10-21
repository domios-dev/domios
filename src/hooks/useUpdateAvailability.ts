import { useCallback, useEffect, useState } from "react";
import { listen } from "../lib/bridge";
import { checkUpdateAvailability, type UpdateAvailabilitySnapshot } from "../lib/updateAvailability";
import { DEV_BUILD } from "../lib/devBuild";

function debugFailure(failure: unknown) {
  if (DEV_BUILD) console.debug("Background update check failed", failure);
}

function logChannelErrors(snapshot: UpdateAvailabilitySnapshot) {
  for (const [channel, detail] of Object.entries(snapshot.errors)) {
    debugFailure(new Error(`Plugin channel '${channel}' refresh failed: ${detail}`));
  }
}

const EMPTY: UpdateAvailabilitySnapshot = { items: [], errors: {} };

export function useUpdateAvailability() {
  const [snapshot, setSnapshot] = useState(EMPTY);
  const [ready, setReady] = useState(false);
  const refresh = useCallback(async (force = false) => {
    try {
      const next = await checkUpdateAvailability(force);
      setSnapshot(next);
      setReady(true);
      logChannelErrors(next);
      return next;
    } catch (failure) {
      debugFailure(failure);
      throw failure;
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void listen<UpdateAvailabilitySnapshot>("update-availability-changed", event => {
      if (!disposed) {
        setSnapshot(event.payload); setReady(true);
        logChannelErrors(event.payload);
      }
    }).then(unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      void checkUpdateAvailability().then(next => {
        if (!disposed) {
          setSnapshot(next); setReady(true);
          logChannelErrors(next);
        }
      }).catch(failure => { if (!disposed) debugFailure(failure); });
    }).catch(failure => { if (!disposed) debugFailure(failure); });
    return () => { disposed = true; stop?.(); };
  }, []);

  return { snapshot, setSnapshot, ready, refresh };
}

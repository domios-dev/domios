import { useCallback, useEffect, useRef, useState, type SetStateAction } from "react";
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
  const mounted = useRef(false);
  const revision = useRef(0);
  const publish = useCallback((next: SetStateAction<UpdateAvailabilitySnapshot>) => {
    revision.current++;
    setSnapshot(next);
  }, []);
  const refresh = useCallback(async (force = false) => {
    const requestRevision = ++revision.current;
    try {
      const next = await checkUpdateAvailability(force);
      if (mounted.current && requestRevision === revision.current) {
        setSnapshot(next);
        setReady(true);
        logChannelErrors(next);
      }
      return next;
    } catch (failure) {
      debugFailure(failure);
      throw failure;
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    let stop: (() => void) | undefined;
    void listen<UpdateAvailabilitySnapshot>("update-availability-changed", event => {
      if (!disposed) {
        publish(event.payload); setReady(true);
        logChannelErrors(event.payload);
      }
    }).then(unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      void refresh().catch(() => {});
    }).catch(failure => { if (!disposed) debugFailure(failure); });
    const signal = () => { void refresh().catch(() => {}); };
    const visible = () => { if (!document.hidden) signal(); };
    window.addEventListener("focus", signal);
    window.addEventListener("online", signal);
    window.addEventListener("pageshow", signal);
    document.addEventListener("visibilitychange", visible);
    document.addEventListener("resume", signal);
    return () => {
      disposed = true;
      mounted.current = false;
      revision.current++;
      stop?.();
      window.removeEventListener("focus", signal);
      window.removeEventListener("online", signal);
      window.removeEventListener("pageshow", signal);
      document.removeEventListener("visibilitychange", visible);
      document.removeEventListener("resume", signal);
    };
  }, [publish, refresh]);

  return { snapshot, setSnapshot: publish, ready, refresh };
}

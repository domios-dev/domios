import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import { getCachedConfig, loadConfig, setUiSettings } from "../../lib/clientSettings";
import type { KeepAwakeStatus } from "../../../gen/KeepAwakeStatus";
import { changeKeepAwake, type KeepAwakeAction, type KeepAwakePorts } from "./keepAwakeActions";

const ports: KeepAwakePorts = {
  status: () => invoke<KeepAwakeStatus>("keepawake_status"),
  start: (durationSecs, display) => invoke<KeepAwakeStatus>("keepawake_start", { durationSecs, display }),
  stop: () => invoke<void>("keepawake_stop"),
  saveDisplay: (display) => setUiSettings({ keepawake_display: display }),
  now: Date.now,
};

function failureMessage(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

export function useKeepAwake(open: boolean) {
  const [status, setStatus] = useState<KeepAwakeStatus>({ active: false, display: false, expires_at: null, gnome_policy: false, system_block: false });
  const [displaySleep, setDisplaySleep] = useState(() => getCachedConfig()?.ui?.keepawake_display === true);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const revision = useRef(0);
  const readSequence = useRef(0);
  const busy = useRef(false);
  const mounted = useRef(false);
  const preferenceTouched = useRef(false);

  const applyStatus = useCallback((next: KeepAwakeStatus) => {
    setStatus(next);
    if (next.active) { preferenceTouched.current = true; setDisplaySleep(next.display); }
  }, []);

  const refresh = useCallback(async () => {
    const version = revision.current;
    const sequence = ++readSequence.current;
    const next = await ports.status();
    if (mounted.current && revision.current === version && readSequence.current === sequence) applyStatus(next);
  }, [applyStatus]);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void loadConfig().then(config => {
      if (!cancelled && !preferenceTouched.current) setDisplaySleep(config.ui?.keepawake_display === true);
    }).catch(() => {});
    void (async () => {
      try {
        const release = await listen<KeepAwakeStatus>("keepawake-changed", event => {
          if (cancelled) return;
          revision.current += 1;
          applyStatus(event.payload);
        });
        if (cancelled) { release(); return; }
        unlisten = release;
        await refresh();
      } catch (failure) {
        if (!cancelled) setError(failureMessage(failure));
      }
    })();
    return () => { cancelled = true; mounted.current = false; revision.current += 1; unlisten?.(); };
  }, [applyStatus, refresh]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void refresh().catch(failure => { if (!cancelled) setError(failureMessage(failure)); });
    return () => { cancelled = true; };
  }, [open, refresh]);

  async function change(action: KeepAwakeAction): Promise<boolean> {
    if (busy.current) {
      setError("A Keep Awake change is still in progress.");
      return false;
    }
    busy.current = true;
    preferenceTouched.current = true;
    setPending(true);
    setError(null);
    const version = ++revision.current;
    try {
      const result = await changeKeepAwake(action, ports);
      if (mounted.current && revision.current === version) {
        applyStatus(result.status);
        if (result.displayPreference !== undefined) setDisplaySleep(result.displayPreference);
      }
      revision.current += 1;
      return true;
    } catch (failure) {
      if (mounted.current) setError(failureMessage(failure));
      try { await refresh(); } catch { /* Preserve the original refusal. */ }
      return false;
    } finally {
      busy.current = false;
      if (mounted.current) setPending(false);
    }
  }

  return { status, displaySleep, error, pending, change };
}

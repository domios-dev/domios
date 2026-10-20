import { useCallback, useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../lib/devBuild";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";
import type { ProviderCliStatus } from "../lib/providerCli";

export function useProviderCliStatus(providerId: string) {
  const [status, setStatus] = useState<ProviderCliStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [probeFailed, setProbeFailed] = useState(false);
  const [summary, setSummary] = useState<string | null>(null);
  const sequence = useRef(0);
  const refreshStatus = useCallback(async (userInitiated: boolean) => {
    const request = ++sequence.current;
    try {
      const next = await invoke<ProviderCliStatus>("provider_cli_status", { providerId });
      if (sequence.current === request) {
        setStatus(next);
        setProbeFailed(false);
        if (next.update_summary != null) setSummary(next.update_summary);
        if (userInitiated) setError(null);
      }
    } catch (error) {
      if (sequence.current === request) {
        setProbeFailed(true);
        if (userInitiated) setError(String(error));
        else if (DEV_BUILD) console.debug("Background provider CLI status refresh failed", error);
      }
    }
  }, [providerId]);
  const refresh = useCallback(() => refreshStatus(true), [refreshStatus]);

  useEffect(() => {
    let active = true;
    const unlisteners: UnlistenFn[] = [];
    setStatus(null);
    setSummary(null);
    setError(null);
    setProbeFailed(false);
    for (const event of ["provider-cli-completed", "provider-cli-started", "provider-cli-update-available"]) void listen<{ provider: string; summary?: string }>(event, ({ payload }) => {
      if (active && payload.provider === providerId) { if (payload.summary) setSummary(payload.summary); void refreshStatus(false); }
    }).then((stop) => { if (active) unlisteners.push(stop); else stop(); }).catch((error) => {
      if (active && DEV_BUILD) console.debug("Provider CLI status listener unavailable", error);
    });
    void refreshStatus(false);
    const onFocus = () => void refreshStatus(false);
    window.addEventListener("focus", onFocus);
    return () => { active = false; sequence.current++; unlisteners.forEach(stop => stop()); window.removeEventListener("focus", onFocus); };
  }, [providerId, refreshStatus]);

  return { status, error, probeFailed, summary, refresh };
}

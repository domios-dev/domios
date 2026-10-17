import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";
import type { ProviderCliStatus } from "../lib/providerCli";

export function useProviderCliStatus(providerId: string) {
  const [status, setStatus] = useState<ProviderCliStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const sequence = useRef(0);
  const refresh = useCallback(async () => {
    const request = ++sequence.current;
    try {
      const next = await invoke<ProviderCliStatus>("provider_cli_status", { providerId });
      if (sequence.current === request) { setStatus(next); if (next.update_summary != null) setSummary(next.update_summary); setError(null); }
    } catch (error) {
      if (sequence.current === request) { setStatus(null); setError(String(error)); }
    }
  }, [providerId]);

  useEffect(() => {
    let active = true;
    const unlisteners: UnlistenFn[] = [];
    setStatus(null);
    setSummary(null);
    for (const event of ["provider-cli-completed", "provider-cli-started", "provider-cli-update-available"]) void listen<{ provider: string; summary?: string }>(event, ({ payload }) => {
      if (active && payload.provider === providerId) { if (payload.summary) setSummary(payload.summary); void refresh(); }
    }).then((stop) => { if (active) unlisteners.push(stop); else stop(); }).catch((error) => {
      if (active) setError(String(error));
    });
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => { active = false; sequence.current++; unlisteners.forEach(stop => stop()); window.removeEventListener("focus", onFocus); };
  }, [providerId, refresh]);

  return { status, error, summary, refresh };
}

import { useCallback, useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../lib/devBuild";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";
import { peerEventRail } from "../lib/peerEventRail";
import type { ProviderCliStatus } from "../lib/providerCli";

export function useProviderCliStatus(providerId: string, peerId?: string | null) {
  const [status, setStatus] = useState<ProviderCliStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [probeFailed, setProbeFailed] = useState(false);
  const [eventSummary, setEventSummary] = useState<string | null>(null);
  const sequence = useRef(0);
  const targetPeerId = typeof peerId === "string" && peerId ? peerId : undefined;
  const refreshStatus = useCallback(async (userInitiated: boolean) => {
    const request = ++sequence.current;
    try {
      const next = await invoke<ProviderCliStatus>("provider_cli_status", {
        providerId,
        ...(targetPeerId !== undefined ? { peerId: targetPeerId } : {}),
        ...(userInitiated ? { refresh: true } : {}),
      });
      if (sequence.current === request) {
        setStatus(next);
        setProbeFailed(false);
        if (userInitiated) {
          setEventSummary(null);
          setError(null);
        }
      }
    } catch (error) {
      if (sequence.current === request) {
        setProbeFailed(true);
        if (userInitiated) setError(String(error));
        else if (DEV_BUILD) console.debug("Background provider CLI status refresh failed", error);
      }
    }
  }, [providerId, targetPeerId]);
  const refresh = useCallback(() => refreshStatus(true), [refreshStatus]);

  useEffect(() => {
    let active = true;
    const unlisteners: UnlistenFn[] = [];
    setStatus(null);
    setEventSummary(null);
    setError(null);
    setProbeFailed(false);
    if (targetPeerId !== undefined) {
      unlisteners.push(peerEventRail.observe(targetPeerId, (event) => {
        if (!active) return;
        if (
          event.type === "provider-cli-started"
          || event.type === "provider-cli-completed"
          || event.type === "provider-cli-update-available"
        ) {
          if (event.data.provider !== providerId) return;
          if (event.data.summary) setEventSummary(event.data.summary);
          void refreshStatus(false);
        } else if (event.type === "reconcile") {
          void refreshStatus(false);
        }
      }));
    } else {
      for (const event of ["provider-cli-completed", "provider-cli-started", "provider-cli-update-available"]) void listen<{ provider: string; summary?: string }>(event, ({ payload }) => {
        if (active && payload.provider === providerId) { if (payload.summary) setEventSummary(payload.summary); void refreshStatus(false); }
      }).then((stop) => { if (active) unlisteners.push(stop); else stop(); }).catch((error) => {
        if (active && DEV_BUILD) console.debug("Provider CLI status listener unavailable", error);
      });
      const onPeerReset = () => {
        sequence.current++;
        setStatus(null);
        setEventSummary(null);
        setError(null);
        setProbeFailed(false);
      };
      const onPeerRebind = () => { void refreshStatus(false); };
      window.addEventListener("codeterm-peer-switch-reset", onPeerReset);
      window.addEventListener("codeterm-peer-switch-rebind", onPeerRebind);
      unlisteners.push(() => {
        window.removeEventListener("codeterm-peer-switch-reset", onPeerReset);
        window.removeEventListener("codeterm-peer-switch-rebind", onPeerRebind);
      });
    }
    void refreshStatus(false);
    const onFocus = () => void refreshStatus(false);
    window.addEventListener("focus", onFocus);
    return () => { active = false; sequence.current++; unlisteners.forEach(stop => stop()); window.removeEventListener("focus", onFocus); };
  }, [providerId, targetPeerId, refreshStatus]);

  const summary = eventSummary ?? status?.update_summary ?? null;
  return { status, error, probeFailed, summary, refresh };
}

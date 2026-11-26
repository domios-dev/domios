import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { peerEventRail } from "../../lib/peerEventRail";
import type { AppConfig } from "../../lib/types";
import { relayTarget } from "../../lib/webApi";
import { useWorkspaceBrowseTarget, type WorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";

/** Returns the browsed remote machine when Settings targets a peer other than the active web relay target. */
export function useSettingsRemoteTarget(): WorkspaceBrowseTarget | null {
  const browseTarget = useWorkspaceBrowseTarget();
  const relay = relayTarget()?.id;
  return browseTarget && browseTarget.id !== relay ? browseTarget : null;
}

interface LoadedRemoteConfig {
  peerId: string;
  config: AppConfig;
  generation: number;
}

export interface SettingsConfigState {
  config: AppConfig;
  onUpdateConfig: (next: AppConfig) => Promise<void>;
  ready: boolean;
  loading: boolean;
  error: unknown;
  retry: () => void;
}

export function useSettingsConfig(
  localConfig: AppConfig,
  onUpdateLocalConfig: (next: AppConfig) => Promise<void>,
  remoteTarget: WorkspaceBrowseTarget | null,
): SettingsConfigState {
  const peerId = remoteTarget?.id ?? null;
  const [loaded, setLoaded] = useState<LoadedRemoteConfig | null>(null);
  const [errorState, setErrorState] = useState<{ peerId: string; error: unknown } | null>(null);
  const [attempt, setAttempt] = useState(0);
  const generationRef = useRef(0);
  const requestVersionRef = useRef(0);
  const loadedRef = useRef<LoadedRemoteConfig | null>(null);
  loadedRef.current = loaded;

  useEffect(() => {
    const generation = ++generationRef.current;
    requestVersionRef.current++;
    if (!peerId) return;
    let active = true;
    const read = () => {
      const requestVersion = ++requestVersionRef.current;
      void invoke<AppConfig>("get_config", { peerId })
        .then((next) => {
          if (!active || generationRef.current !== generation || requestVersion !== requestVersionRef.current) return;
          if (!next) throw new Error("Config is unavailable");
          const snapshot = { peerId, config: next, generation };
          loadedRef.current = snapshot;
          setLoaded(snapshot);
          setErrorState(null);
        })
        .catch((reason: unknown) => {
          if (!active || generationRef.current !== generation || requestVersion !== requestVersionRef.current) return;
          setErrorState({ peerId, error: reason });
        });
    };
    const stopRail = peerEventRail.observe(peerId, (event) => {
      if (event.type === "config-changed" || event.type === "reconcile") read();
    });
    read();
    return () => {
      active = false;
      generationRef.current++;
      requestVersionRef.current++;
      stopRail();
    };
  }, [peerId, attempt]);

  const currentLoaded = peerId && loaded?.peerId === peerId ? loaded : null;
  const currentError = peerId && !currentLoaded && errorState?.peerId === peerId ? errorState.error : null;

  const updateRemoteConfig = useCallback(
    async (next: AppConfig) => {
      const current = loadedRef.current;
      if (!peerId || !current || current.peerId !== peerId || current.generation !== generationRef.current) {
        throw new Error("Config is not ready for this machine");
      }
      const requestVersion = ++requestVersionRef.current;
      await invoke("update_config", { config: next, peerId });
      if (
        loadedRef.current?.peerId === peerId
        && loadedRef.current.generation === current.generation
        && requestVersion === requestVersionRef.current
      ) {
        const snapshot = { peerId, config: next, generation: current.generation };
        loadedRef.current = snapshot;
        setLoaded(snapshot);
      }
    },
    // Rebind whenever the loaded peer or generation changes so SettingsPanel invalidates stale drafts.
    [peerId, currentLoaded?.generation],
  );

  const retry = useCallback(() => {
    setErrorState(null);
    setAttempt((n) => n + 1);
  }, []);

  if (!peerId) {
    return {
      config: localConfig,
      onUpdateConfig: onUpdateLocalConfig,
      ready: true,
      loading: false,
      error: null,
      retry,
    };
  }

  return {
    config: currentLoaded?.config ?? localConfig,
    onUpdateConfig: updateRemoteConfig,
    ready: currentLoaded !== null,
    loading: currentLoaded === null && currentError === null,
    error: currentError,
    retry,
  };
}

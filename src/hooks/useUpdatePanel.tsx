import { createContext, useCallback, useContext, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { listen } from "../lib/bridge";
import { checkUpdateAvailability, type UpdateAvailabilitySnapshot } from "../lib/updateAvailability";
import { availabilityIntent, hasUpdateBadge, nextUpdateStep, panelItems, readUpdateIntent, type UpdateIntent, UPDATE_INTENT_KEY } from "../lib/updatePanelModel";
import { acceptFrontendUpdate, setFrontendUpdateHeld, frontendUpdateOffer, subscribeFrontendUpdateOffer } from "../lib/frontendUpdate/browser";
import { runPluginSnapshotAction } from "../lib/pluginSnapshotStore";
import { useUpdate } from "./useUpdate";
import { classifyUpdateFailure, classifyUpdateStatus, type UpdateFailure } from "../lib/updateStatusModel";

import { DEV_BUILD } from "../lib/devBuild";

function debugFailure(failure: unknown) {
  if (DEV_BUILD) console.debug("Background update check failed", failure);
}

const EMPTY: UpdateAvailabilitySnapshot = { items: [], errors: {} };

function savedIntent(): UpdateIntent | null {
  return readUpdateIntent(localStorage.getItem(UPDATE_INTENT_KEY));
}

function persistIntent(intent: UpdateIntent | null): void {
  if (intent) localStorage.setItem(UPDATE_INTENT_KEY, JSON.stringify(intent));
  else localStorage.removeItem(UPDATE_INTENT_KEY);
}

function useUpdatePanelController() {
  const app = useUpdate();
  const [snapshot, setSnapshot] = useState(EMPTY);
  const [intent, setIntent] = useState(savedIntent);
  const [error, setError] = useState<UpdateFailure | null>(null);
  const [starting, setStarting] = useState(false);
  const [ready, setReady] = useState(false);
  const running = useRef(false);
  const offer = useSyncExternalStore(subscribeFrontendUpdateOffer, frontendUpdateOffer, () => null);
  const items = panelItems(snapshot.items);
  const status = classifyUpdateStatus(snapshot);

  const refresh = useCallback(async (force = false) => {
    try {
      const next = await checkUpdateAvailability(force);
      setSnapshot(next);
      setReady(true);
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
        for (const failure of Object.values(classifyUpdateStatus(event.payload).errors)) debugFailure(failure);
      }
    }).then(unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      void checkUpdateAvailability().then(next => {
        if (!disposed) {
          setSnapshot(next); setReady(true);
          for (const failure of Object.values(classifyUpdateStatus(next).errors)) debugFailure(failure);
        }
      }).catch(failure => { if (!disposed) debugFailure(failure); });
    }).catch(failure => { if (!disposed) debugFailure(failure); });
    return () => { disposed = true; stop?.(); };
  }, []);

  const save = useCallback((next: UpdateIntent | null) => {
    persistIntent(next);
    setFrontendUpdateHeld(next !== null);
    setIntent(next);
  }, []);

  const hostVersion = snapshot.items.find(item => item.kind === "app")?.installed;
  const nextStep = ready && intent && !error && Object.keys(status.errors).length === 0 && hostVersion
    ? nextUpdateStep(intent, hostVersion, items) : null;
  const stepKind = nextStep?.kind === "interface" && !offer ? "waitForApp" : nextStep?.kind;
  const pluginId = nextStep?.kind === "plugin" ? nextStep.id : null;
  const intentKey = intent ? JSON.stringify(intent) : null;

  useEffect(() => {
    const currentIntent = readUpdateIntent(intentKey);
    if (!currentIntent || !stepKind || stepKind === "waitForApp" || running.current) return;
    running.current = true;
    void (async () => {
      try {
        if (stepKind === "plugin" && pluginId) {
          await runPluginSnapshotAction({ kind: "update", pluginId });
          await refresh(true);
          running.current = false;
          save({ ...currentIntent, stage: "plugins", pluginIds: currentIntent.pluginIds.filter(id => id !== pluginId) });
        } else if (stepKind === "interface") {
          save(null);
          acceptFrontendUpdate();
        } else save(null);
      } catch (failure) { setError(classifyUpdateFailure("update", failure)); }
      finally { running.current = false; }
    })();
  }, [intentKey, stepKind, pluginId, refresh, save]);

  const updateAll = async () => {
    if (intent || running.current || app.installing) return;
    running.current = true;
    setStarting(true);
    setError(null);
    try {
      const fresh = await refresh(true);
      if (Object.keys(classifyUpdateStatus(fresh).errors).length > 0) throw new Error(Object.values(classifyUpdateStatus(fresh).errors).join("\n"));
      const next = availabilityIntent(fresh.items);
      running.current = false;
      save(next);
      if (next.appTarget) {
        const result = await app.applyUpdate({ failureSurface: "inline" });
        if (!result) throw new Error("App update failed");
      }
    } catch (failure) { setError(classifyUpdateFailure("update", failure)); }
    finally { running.current = false; setStarting(false); }
  };

  const applyItem = async (id: string) => {
    if (intent || running.current || app.installing) return;
    const item = snapshot.items.find(item => item.id === id);
    if (!item || !hasUpdateBadge(item)) return;
    running.current = true;
    setStarting(true);
    setError(null);
    try {
      save(availabilityIntent([item]));
      if (item.kind === "app" && !await app.applyUpdate({ failureSurface: "inline" })) throw new Error("App update failed");
    } catch (failure) { setError(classifyUpdateFailure("update", failure)); }
    finally { running.current = false; setStarting(false); }
  };

  const retry = async () => {
    setError(null);
    try {
      const fresh = await refresh(true);
      const failures = Object.values(classifyUpdateStatus(fresh).errors);
      if (failures.length) throw new Error(failures.join("\n"));
      const host = fresh.items.find(item => item.kind === "app")?.installed;
      if (intent?.stage === "app" && host !== intent.appTarget && !await app.applyUpdate({ failureSurface: "inline" })) {
        throw new Error("App update failed");
      }
    } catch (failure) { setError(classifyUpdateFailure("check", failure)); }
  };

  return { snapshot, items, information: status.information, busy: starting || intent !== null || app.installing, error: error?.message ?? (app.error ? classifyUpdateFailure("update", app.error)?.message : null),
    errorDetails: error ? [error.details, app.error].filter(Boolean).join("\n") : app.error,
    updateAll, applyItem, retry, refresh, pluginHasUpdate: (id: string) => hasUpdateBadge(snapshot.items.find(item => item.kind === "plugin" && item.id === id)) };
}

const Context = createContext<ReturnType<typeof useUpdatePanelController> | null>(null);
export function UpdatePanelProvider({ children }: { children: ReactNode }) {
  return <Context.Provider value={useUpdatePanelController()}>{children}</Context.Provider>;
}
export function useUpdatePanel() {
  const value = useContext(Context);
  if (!value) throw new Error("useUpdatePanel requires UpdatePanelProvider");
  return value;
}
export function useOptionalUpdatePanel() { return useContext(Context); }

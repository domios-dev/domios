import { createContext, useCallback, useContext, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { useUpdateAvailability } from "./useUpdateAvailability";
import { targetInstalled, updateRunRows, availabilityIntent, hasUpdateBadge, nextUpdateStep, panelItems, readUpdateIntent, updateableItems, type UpdateIntent, type UpdateItem, UPDATE_INTENT_KEY } from "../lib/updatePanelModel";
import { acceptFrontendUpdate, setFrontendUpdateHeld, frontendUpdateOffer, subscribeFrontendUpdateOffer } from "../lib/frontendUpdate/browser";
import { runPluginSnapshotAction } from "../lib/pluginSnapshotStore";
import { useUpdate } from "./useUpdate";
import { channelDiagnostics, classifyUpdateFailure, classifyUpdateStatus, type UpdateFailure } from "../lib/updateStatusModel";

function savedIntent(): UpdateIntent | null {
  return readUpdateIntent(localStorage.getItem(UPDATE_INTENT_KEY));
}

function persistIntent(intent: UpdateIntent | null): void {
  if (intent) localStorage.setItem(UPDATE_INTENT_KEY, JSON.stringify(intent));
  else localStorage.removeItem(UPDATE_INTENT_KEY);
}

function useUpdatePanelController() {
  const app = useUpdate();
  const { snapshot, setSnapshot, ready, refresh } = useUpdateAvailability();
  const [intent, setIntent] = useState(savedIntent);
  const [error, setError] = useState<UpdateFailure | null>(null);
  const [history, setHistory] = useState<UpdateFailure[]>([]);
  const [starting, setStarting] = useState(false);
  const [checking, setChecking] = useState(false);
  const [checkResult, setCheckResult] = useState<string | null>(null);
  const [runItems, setRunItems] = useState<UpdateItem[]>([]);
  const [outcomes, setOutcomes] = useState<Record<string, "completed" | "failed">>({});
  const running = useRef(false);
  const offer = useSyncExternalStore(subscribeFrontendUpdateOffer, frontendUpdateOffer, () => null);
  const status = classifyUpdateStatus(snapshot);
  const items = panelItems(snapshot.items, status.errors);
  const availableRows = useRef(items);
  availableRows.current = items;
  const updateable = panelItems(updateableItems(snapshot.items, status.errors));
  const diagnostics = channelDiagnostics(status.errors, new Set(items.flatMap(item => item.channelError && item.channel ? [item.channel] : [])));

  const save = useCallback((next: UpdateIntent | null) => {
    persistIntent(next);
    setFrontendUpdateHeld(next !== null);
    setIntent(next);
  }, []);

  const hostVersion = snapshot.items.find(item => item.kind === "app")?.installed ?? app.currentVersion ?? "";
  const nextStep = ready && intent && !error
    ? nextUpdateStep(intent, hostVersion, updateable) : null;
  const stepKind = nextStep?.kind === "interface" && !offer ? "waitForApp" : nextStep?.kind;
  const pluginId = nextStep?.kind === "plugin" ? nextStep.id : null;
  const pluginChannel = pluginId
    ? snapshot.items.find(item => item.kind === "plugin" && item.id === pluginId)?.channel ?? null
    : null;
  const intentKey = intent ? JSON.stringify(intent) : null;

  const recordOutcome = useCallback((id: string, outcome: "completed" | "failed") => {
    setOutcomes(previous => ({ ...previous, [id]: outcome }));
  }, []);

  useEffect(() => {
    const currentIntent = readUpdateIntent(intentKey);
    if (!currentIntent || !stepKind || stepKind === "waitForApp" || running.current) return;
    running.current = true;
    const activeRow = availableRows.current.find(item => item.id === pluginId);
    setRunItems(previous => previous.length ? previous : availableRows.current);
    void (async () => {
      try {
        if (stepKind === "plugin" && pluginId) {
          await runPluginSnapshotAction({ kind: "update", pluginId });
          recordOutcome(pluginId, "completed");
          await refresh(true);
          running.current = false;
          save({ ...currentIntent, stage: "plugins", pluginIds: currentIntent.pluginIds.filter(id => id !== pluginId) });
        } else if (stepKind === "interface") {
          recordOutcome("frontend", "completed");
          save(null);
          acceptFrontendUpdate();
        } else { await refresh(true); save(null); }
      } catch (failure) {
        if (pluginId) {
          const fresh = await refresh(true).catch(() => null);
          if (fresh && activeRow && targetInstalled(activeRow, fresh.items)) {
            recordOutcome(pluginId, "completed");
            save({ ...currentIntent, stage: "plugins", pluginIds: currentIntent.pluginIds.filter(id => id !== pluginId) });
            return;
          }
        }
        if (pluginId) recordOutcome(pluginId, "failed");
        const classified = classifyUpdateFailure("update", failure);
        if (classified?.channelUnavailable && pluginId && pluginChannel) {
          setSnapshot(previous => ({
            ...previous,
            errors: { ...previous.errors, [pluginChannel]: classified.message },
          }));
          save({ ...currentIntent, stage: "plugins", pluginIds: currentIntent.pluginIds.filter(id => id !== pluginId) });
        } else setError(classified);
      }
      finally { running.current = false; }
    })();
  }, [intentKey, stepKind, pluginId, pluginChannel, refresh, save, recordOutcome]);

  useEffect(() => {
    if (app.error && intent?.stage === "app") { recordOutcome("app", "failed"); save(null); }
  }, [app.error, intent?.stage, recordOutcome, save]);

  const updateAll = async () => {
    if (intent || running.current || app.installing) return;
    running.current = true;
    setStarting(true);
    setError(null);
    setRunItems(items.filter(item => !item.channelError));
    setOutcomes({});
    try {
      const fresh = await refresh(true);
      const freshErrors = classifyUpdateStatus(fresh).errors;
      const next = availabilityIntent(updateableItems(fresh.items, freshErrors));
      running.current = false;
      setRunItems(previous => [...previous, ...panelItems(updateableItems(fresh.items, freshErrors)).filter(item => !previous.some(row => row.id === item.id))]);
      save(next);
      if (next.appTarget) {
        const result = await app.applyUpdate({ failureSurface: "inline" });
        if (!result) throw new Error(app.error ?? "App update failed");
        if (result.status !== "updating") {
          if (result.status === "up_to_date") recordOutcome("app", "completed");
          save(null);
        }
      }
    } catch (failure) { recordOutcome("app", "failed"); save(null); setError(classifyUpdateFailure("update", failure)); }
    finally { running.current = false; setStarting(false); }
  };

  const applyItem = async (id: string) => {
    if (intent || running.current || app.installing) return;
    const row = items.find(item => item.id === id);
    const item = snapshot.items.find(item => item.id === id);
    if (!item || row?.channelError || !hasUpdateBadge(item)) return;
    running.current = true;
    setStarting(true);
    setError(null);
    try {
      setRunItems(panelItems([item]));
      setOutcomes({});
      save(availabilityIntent([item]));
      if (item.kind === "app") {
        const result = await app.applyUpdate({ failureSurface: "inline" });
        if (!result) throw new Error(app.error ?? "App update failed");
        if (result.status !== "updating") {
          if (result.status === "up_to_date") recordOutcome("app", "completed");
          save(null);
        }
      }
    } catch (failure) { recordOutcome(id, "failed"); save(null); setError(classifyUpdateFailure("update", failure)); }
    finally { running.current = false; setStarting(false); }
  };

  const retry = async () => {
    if (checking || ((starting || intent || app.installing) && !error && !app.error)) return;
    setChecking(true);
    setCheckResult(null);
    const previousFailure = error ?? (app.error ? classifyUpdateFailure("update", app.error) : null);
    if (previousFailure) setHistory(previous => [...previous, previousFailure].slice(-10));
    save(null);
    setError(null);
    try {
      const fresh = await refresh(true);
      const available = updateableItems(fresh.items, classifyUpdateStatus(fresh).errors);
      const failures = Object.keys(fresh.errors).length;
      setCheckResult(failures
        ? `Check finished with ${failures} unavailable update source(s).`
        : available.length ? `Check complete: ${available.length} update(s) available.` : "Check complete: everything is up to date.");
    } catch (failure) { setError(classifyUpdateFailure("check", failure)); }
    finally { setChecking(false); }
  };

  const activeId = stepKind === "plugin" ? pluginId : stepKind === "interface" ? "frontend"
    : intent?.stage === "app" && hostVersion !== intent.appTarget ? "app" : null;
  const rows = updateRunRows(runItems, items, snapshot.items, intent, activeId, outcomes);
  const failure = error?.message ?? (app.error ? classifyUpdateFailure("update", app.error)?.message : null);
  return { snapshot, items, rows, activeId, awaitingRetry: Boolean(intent && failure), appProgress: app.status === "downloading" && app.downloadTotalBytes && app.downloadTotalBytes > 0
    ? Math.min(100, Math.max(0, app.downloadedBytes / app.downloadTotalBytes * 100)) : null,
    history, checking, checkResult, information: status.information, diagnostics, busy: starting || (!failure && intent !== null) || app.installing, error: failure,
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

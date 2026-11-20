import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "../../lib/bridge";
import { shouldWakeOnOpen } from "../../lib/wakeOnOpen";
import { restoreStageView, type WakeRequestView } from "../../lib/restoreStageView";
import { OutcomeUnknown, RetryableMuxError } from "../../lib/streamMux";
import type { Tab } from "../../lib/types";

const idle: WakeRequestView = { requested: false, pending: false, error: null };
const requests = new Map<string, WakeRequestView>();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
function update(id: string, state: WakeRequestView) {
  if (state === idle) requests.delete(id);
  else requests.set(id, state);
  for (const listener of listeners) listener();
}

function requestWake(id: string, retry = false) {
  const current = requests.get(id) ?? idle;
  if (current.pending || (current.requested && !retry)) return;
  update(id, { requested: true, pending: true, error: null });
  void invoke("wake_tab", { tabId: id }).then(
    () => update(id, { requested: true, pending: false, error: null }),
    (error: unknown) => {
      const interrupted = error instanceof OutcomeUnknown
        || error instanceof RetryableMuxError && error.dispatched !== false
        || error instanceof DOMException && error.name === "AbortError";
      update(id, { requested: true, pending: false,
        error: interrupted ? null : error instanceof Error ? error.message : String(error) });
    },
  );
}

export function useWakeOnOpen(tab: Tab | undefined, isVisible: boolean) {
  const id = tab?.id;
  const state = useSyncExternalStore(subscribe, () => id ? requests.get(id) ?? idle : idle, () => idle);
  useEffect(() => {
    if (!id) return;
    if (shouldWakeOnOpen(tab, isVisible, state.requested)) requestWake(id);
    const backendAttached = tab?.backing?.kind !== "pty" || tab.pty_id != null;
    if (backendAttached && !tab?.frozen && state.requested && !state.pending && restoreStageView(tab ?? {}, state).phase === "ready") {
      update(id, idle);
    }
  }, [id, tab, isVisible, state]);
  return { ...state, retry: () => { if (id) requestWake(id, true); } };
}

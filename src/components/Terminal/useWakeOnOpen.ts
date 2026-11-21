import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "../../lib/bridge";
import { shouldWakeOnOpen } from "../../lib/wakeOnOpen";
import { restoreStageView, type WakeRequestView } from "../../lib/restoreStageView";
import { DEV_BUILD } from "../../lib/devBuild";
import { PeerTabRelayError } from "../../lib/remoteChatState";
import { TransportError } from "../../lib/transportClient";
import type { Tab } from "../../lib/types";

const idle: WakeRequestView = { requested: false, pending: false, refusal: null };
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
  update(id, { requested: true, pending: true, refusal: null });
  void invoke("wake_tab", { tabId: id }).then(
    () => update(id, { requested: true, pending: false, refusal: null }),
    (error: unknown) => {
      const refused = (error instanceof PeerTabRelayError && error.status === 403 && error.code === "peer_relay_not_authorized")
        || (error instanceof TransportError && error.status === 403 && error.path === `/api/tabs/${encodeURIComponent(id)}/wake`);
      if (DEV_BUILD) console.debug("[tab-wake] request rejected", {
        tabId: id, kind: error instanceof DOMException ? error.name : error instanceof Error ? error.constructor.name : typeof error,
        status: error instanceof TransportError || error instanceof PeerTabRelayError ? error.status : undefined,
        refused,
      });
      update(id, { requested: true, pending: false, refusal: refused ? error.message : null });
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

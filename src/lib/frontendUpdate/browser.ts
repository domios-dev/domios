import { readUpdateIntent, UPDATE_INTENT_KEY } from "../updatePanelModel";
import type { TransportClient } from "../transportClient";
import { restoreComposerDrafts } from "./drafts";
import { setPersistedChatScrollState, type ChatScrollState } from "../../components/Terminal/chatViewport";
import { frontendSignals, setCheckpointRestorer } from "./appState";
import { initializeFrontendUpdateRuntime, ROOT_RENDERED_EVENT, type FrontendRuntimeOptions, type FrontendUpdateOffer } from "./runtime";

let runtime: Awaited<ReturnType<typeof initializeFrontendUpdateRuntime>> | null = null;
let held = typeof localStorage !== "undefined" && typeof localStorage.getItem === "function" && readUpdateIntent(localStorage.getItem(UPDATE_INTENT_KEY)) !== null;
let offer: FrontendUpdateOffer | null = null;
const offerListeners = new Set<() => void>();

export function frontendRootRendered(): void { runtime?.rootRendered(); }
export function frontendUpdateOffer(): FrontendUpdateOffer | null { return offer; }
export function subscribeFrontendUpdateOffer(listener: () => void): () => void {
  offerListeners.add(listener);
  return () => { offerListeners.delete(listener); };
}
export function setFrontendUpdateHeld(next: boolean): void { held = next; runtime?.hold(next); }
export function acceptFrontendUpdate(): void { runtime?.consent(); setFrontendUpdateHeld(false); }

function publishOffer(next: FrontendUpdateOffer | null): void {
  if (next?.releaseId === offer?.releaseId) return;
  offer = next;
  for (const listener of offerListeners) listener();
}

export async function initializeBrowserFrontendUpdates(client: TransportClient, context: Pick<FrontendRuntimeOptions, "clientId" | "boot" | "bundled" | "onError">) {
  setCheckpointRestorer(value => {
    const restored = restoreComposerDrafts(value, localStorage);
    for (const [key, scroll] of Object.entries(restored.scroll)) {
      if (scroll.chat) setPersistedChatScrollState(key, JSON.parse(scroll.chat) as ChatScrollState);
    }
    return restored;
  });
  const syncFocus = () => frontendSignals.signal({ focused: document.hasFocus(),
    typing: document.activeElement instanceof HTMLTextAreaElement || (document.activeElement instanceof HTMLElement && document.activeElement.isContentEditable) });
  const syncModals = () => frontendSignals.signal({ modalOpen: document.querySelector('[role="dialog"], [role="alertdialog"], dialog[open]') !== null });
  const startComposition = () => frontendSignals.signal({ composing: true });
  const endComposition = () => frontendSignals.signal({ composing: false });
  syncFocus(); syncModals();
  window.addEventListener("focus", syncFocus); window.addEventListener("blur", syncFocus);
  document.addEventListener("focusin", syncFocus); document.addEventListener("focusout", syncFocus);
  document.addEventListener("compositionstart", startComposition); document.addEventListener("compositionend", endComposition);
  const modals = new MutationObserver(syncModals);
  modals.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["role", "open"] });
  try {
    runtime = await initializeFrontendUpdateRuntime(client, { ...context, held: () => held, signals: frontendSignals, storage: sessionStorage,
      navigate: path => location.assign(path), events: window, onOffer: publishOffer,
      event: detail => new CustomEvent(ROOT_RENDERED_EVENT, { detail }),
    });
  } catch (error) {
    cleanup(); throw error;
  }
  function cleanup() {
    modals.disconnect();
    window.removeEventListener("focus", syncFocus); window.removeEventListener("blur", syncFocus);
    document.removeEventListener("focusin", syncFocus); document.removeEventListener("focusout", syncFocus);
    document.removeEventListener("compositionstart", startComposition); document.removeEventListener("compositionend", endComposition);
    runtime?.dispose(); runtime = null;
  }
  return { dispose: cleanup };
}

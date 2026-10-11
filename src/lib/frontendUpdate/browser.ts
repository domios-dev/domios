import type { TransportClient } from "../transportClient";
import { COMPOSER_DRAFT_PREFIX } from "../composerDraftGc";
import { setPersistedChatScrollState, type ChatScrollState } from "../../components/Terminal/chatViewport";
import { frontendSignals, setCheckpointRestorer } from "./appState";
import { initializeFrontendUpdateRuntime, ROOT_RENDERED_EVENT, type FrontendRuntimeOptions } from "./runtime";

let runtime: Awaited<ReturnType<typeof initializeFrontendUpdateRuntime>> | null = null;

export function frontendRootRendered(): void { runtime?.rootRendered(); }

export async function initializeBrowserFrontendUpdates(client: TransportClient, context: Pick<FrontendRuntimeOptions, "clientId" | "boot" | "bundled" | "onError">) {
  const checkpoint = frontendSignals.checkpoint();
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    if (key?.startsWith(COMPOSER_DRAFT_PREFIX)) checkpoint.drafts[key.slice(COMPOSER_DRAFT_PREFIX.length)] = {
      text: localStorage.getItem(key) ?? "", attachmentIds: [], replyTo: null,
    };
  }
  frontendSignals.edit(checkpoint);
  setCheckpointRestorer(value => {
    for (const [scope, draft] of Object.entries(value.drafts)) localStorage.setItem(COMPOSER_DRAFT_PREFIX + scope, draft.text);
    for (const [key, scroll] of Object.entries(value.scroll)) {
      if (scroll.chat) setPersistedChatScrollState(key, JSON.parse(scroll.chat) as ChatScrollState);
    }
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
    runtime = await initializeFrontendUpdateRuntime(client, { ...context, signals: frontendSignals, storage: sessionStorage,
      navigate: path => location.assign(path), events: window,
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

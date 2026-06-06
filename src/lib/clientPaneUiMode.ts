import type { PaneUiMode } from "./chatUiMode";

const PREFIX = "codeterm:pane-ui-mode:";
const EVENT = "codeterm-client-pane-ui-mode";

export function getClientPaneUiMode(paneId: string): PaneUiMode | null {
  try {
    const value = window.sessionStorage.getItem(`${PREFIX}${paneId}`);
    return value === "chat" || value === "terminal" ? value : null;
  } catch {
    return null;
  }
}

export function setClientPaneUiMode(paneId: string, mode: PaneUiMode): void {
  try {
    window.sessionStorage.setItem(`${PREFIX}${paneId}`, mode);
  } catch {
    /* sessionStorage may be disabled; the event still updates this client. */
  }
  window.dispatchEvent(
    new CustomEvent(EVENT, {
      detail: { paneId, mode },
    }),
  );
}

export function listenClientPaneUiMode(
  paneId: string,
  onMode: (mode: PaneUiMode) => void,
): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ paneId?: string; mode?: PaneUiMode }>).detail;
    if (detail?.paneId !== paneId) return;
    if (detail.mode === "chat" || detail.mode === "terminal") onMode(detail.mode);
  };
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}

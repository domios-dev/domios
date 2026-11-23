import { recordClientHealth, type ChatViewportCause } from "../../lib/clientJournal";
import type { ChatViewportMode } from "./chatViewport";

export type { ChatViewportCause };

/** A user scroll is frequent intent; only detaches the reader did not scroll into are journaled. */
export function journalChatViewportMode(tabId: string | undefined, mode: ChatViewportMode, cause: ChatViewportCause): void {
  if (tabId && mode === "detached" && cause !== "scroll") recordClientHealth({ event: "chat-viewport-detached", tabId, elapsedMs: 0, cause });
}

/** Newer rows reached a detached reader on the window's bottom edge; the cause names how it detached. */
export function journalChatNewerRevealed(tabId: string | undefined, cause: ChatViewportCause): void {
  if (tabId) recordClientHealth({ event: "chat-newer-revealed", tabId, elapsedMs: 0, cause });
}

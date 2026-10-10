import { formatAttachmentMarker } from "@codeterm/chat-core";
import { getCurrentTabUiMode, setClientTabUiMode } from "../../lib/clientTabUiMode";

/** A quote the composer will send as a reply; `content` is stored to a temp file the agent reads. */
export interface ReplyRequest {
  label: string;
  excerpt: string;
  /** The quoted message's id, so the sent bubble can jump back to it. */
  ref: string;
  content: string;
}

/** The composer's docked reply once its quote is stored. */
export interface ComposerReply {
  label: string;
  excerpt: string;
  ref: string;
  path: string;
}

const pending = new Map<string, ReplyRequest>();
const listeners = new Set<() => void>();

/** One reply per tab: a newer request replaces the one not yet taken. */
export const composerReplyStore = {
  set(tabId: string, request: ReplyRequest) {
    pending.set(tabId, request);
    for (const listener of listeners) listener();
  },
  take(tabId: string): ReplyRequest | null {
    const request = pending.get(tabId) ?? null;
    pending.delete(tabId);
    return request;
  },
  clear() { pending.clear(); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};

/** Shows the tab's chat and docks the quote above its composer input, replacing any earlier one. */
export function replyInComposer(tabId: string, request: ReplyRequest) {
  if (getCurrentTabUiMode(tabId) !== "chat") setClientTabUiMode(tabId, "chat");
  composerReplyStore.set(tabId, request);
}

export function encodeComposerReply(reply: ComposerReply | null): string {
  return reply ? JSON.stringify(reply) : "";
}

export function decodeComposerReply(raw: string): ComposerReply | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Partial<ComposerReply>;
    if (typeof value.path !== "string" || !value.path || typeof value.label !== "string") return null;
    return { label: value.label, excerpt: typeof value.excerpt === "string" ? value.excerpt : "", ref: typeof value.ref === "string" ? value.ref : "", path: value.path };
  } catch {
    return null;
  }
}

/** The quote attachment a reply sends as, at `path` (a target-local copy for a remote tab). */
export function replyMarker(reply: ComposerReply, path = reply.path): string {
  return formatAttachmentMarker(path, "quote", reply.label, { ref: reply.ref, excerpt: reply.excerpt });
}

/** The message text with the reply leading it on its own line. */
export function withReplyMarker(marker: string, body: string): string {
  return body ? `${marker}\n${body}` : marker;
}

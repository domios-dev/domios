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

/** The composer's docked reply once its quote is stored (or held inline for text-only harnesses). */
export interface ComposerReply {
  label: string;
  excerpt: string;
  ref: string;
  path: string;
  content?: string;
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
    const path = typeof value.path === "string" ? value.path : "";
    const content = typeof value.content === "string" ? value.content : undefined;
    if ((!path && !content) || typeof value.label !== "string") return null;
    return {
      label: value.label,
      excerpt: typeof value.excerpt === "string" ? value.excerpt : "",
      ref: typeof value.ref === "string" ? value.ref : "",
      path,
      ...(content !== undefined ? { content } : {}),
    };
  } catch {
    return null;
  }
}

export function sameComposerReply(left: ComposerReply | null, right: ComposerReply | null): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return left.path === right.path && left.ref === right.ref && left.label === right.label
    && left.excerpt === right.excerpt && left.content === right.content;
}

/** The quote attachment a reply sends as, at `path` (a target-local copy for a remote tab). */
export function replyMarker(reply: ComposerReply, path = reply.path): string {
  return formatAttachmentMarker(path, "quote", reply.label, { ref: reply.ref, excerpt: reply.excerpt });
}

/** The message text with the reply leading it on its own line. */
export function withReplyMarker(marker: string, body: string): string {
  return body ? `${marker}\n${body}` : marker;
}

/** Plain attributed blockquote text for harnesses without file-path attachment reading. */
export function inlineReplyText(reply: ComposerReply): string {
  const content = reply.content?.trim();
  if (content) return content;
  const excerpt = reply.excerpt.trim();
  if (excerpt) {
    const quoted = excerpt.split(/\r?\n/).map((line) => (line.trim() ? `> ${line}` : ">")).join("\n");
    return reply.label ? `${reply.label}:\n${quoted}` : quoted;
  }
  return reply.label ? `> ${reply.label}` : "";
}

/** Combines an inline reply quote ahead of the message body. */
export function withInlineReply(quote: string, body: string): string {
  if (!quote) return body;
  return body ? `${quote}\n\n${body}` : quote;
}

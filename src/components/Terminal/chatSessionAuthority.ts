import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";

export type TranscriptAuthority = Pick<import("../../../gen/ChatStartResult").ChatStartResult, "source_session_id" | "session_binding_nonce">;

export function transcriptAuthorityMatches(
  listener: TranscriptAuthority | null,
  sessionId: string | null,
  nonce: string | null | undefined,
): boolean {
  return listener != null && listener.source_session_id === sessionId
    && (listener.session_binding_nonce ?? null) === (nonce ?? null);
}

export function needsTranscriptAttachment(
  requested: TranscriptAuthority | null,
  sessionId: string | null,
  nonce: string | null | undefined,
  active: boolean,
  listenerReady: boolean,
): boolean {
  return active && listenerReady && !transcriptAuthorityMatches(requested, sessionId, nonce);
}

export type ChatAttachmentState =
  | "idle"
  | "attaching"
  | "awaiting_session"
  | "awaiting_store"
  | "attached"
  | "failed";

export function attachmentStateForSnapshot(
  snapshot: Pick<ChatStartResult, "pending">,
): ChatAttachmentState {
  return snapshot.pending === true ? "awaiting_store" : "attached";
}

export function attachmentActivity(
  state: ChatAttachmentState,
  loading: boolean,
  hadBoundSession: boolean,
  hasBoundSession = hadBoundSession,
): { loading: boolean; recovering: boolean } {
  return {
    loading: state !== "attached" && loading,
    recovering: hadBoundSession && (state === "awaiting_store" || (state === "awaiting_session" && hasBoundSession)),
  };
}

/** Why the host cannot show a transcript for a session it is bound to. Both are
 *  reported states, not waits: an observed delta is what clears either. */
export type ChatUnavailableReason = "no_transcript_source" | "authority_mismatch";

export type ChatStartResult = Omit<import("../../../gen/ChatStartResult").ChatStartResult, "messages" | "archived_messages" | "kind" | "offset_kind" | "start_offset" | "pending" | "source_missing"> & Partial<Pick<import("../../../gen/ChatStartResult").ChatStartResult, "kind" | "offset_kind" | "start_offset" | "pending" | "source_missing">> & {messages?: ChatMessage[]; archived_messages?: ChatMessage[]};

export type KnownChatWindow = import("../../../gen/KnownChatWindow").KnownChatWindow;

export function chatStartBatchKind(kind: ChatStartResult["kind"]): "snapshot" | "delta" | null {
  return kind === "unchanged" ? null : kind === "delta" ? "delta" : "snapshot";
}

export function knownWindowForAuthority(
  known: KnownChatWindow | undefined,
  sessionId: string | null,
  nonce: string | null | undefined,
): KnownChatWindow | undefined {
  return known && transcriptAuthorityMatches(known, sessionId, nonce) ? known : undefined;
}

export type ChatBackfillResult = Omit<import("../../../gen/ChatBackfillResult").ChatBackfillResult, "messages" | "offset_kind"> & Partial<Pick<import("../../../gen/ChatBackfillResult").ChatBackfillResult, "offset_kind">> & {messages?: ChatMessage[]};

export interface ChatMsgEvent extends TranscriptAuthority {
  tab_id: string;
  messages: ChatMessage[];
  cursor?: string;
  offset_kind?: "bytes" | "items";
  start_offset?: number;
  offset?: number;
}

export function canAdoptTranscriptAuthority(
  incoming: TranscriptAuthority,
  tabSessionId: string | null,
  tabSessionBindingNonce: string | null,
  allowUnboundAuthority = false,
): boolean {
  if (tabSessionId == null && !allowUnboundAuthority) return false;
  return transcriptAuthorityMatches(incoming, tabSessionId, tabSessionBindingNonce);
}

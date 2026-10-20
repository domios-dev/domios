import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";

export interface TranscriptAuthority {
  source_session_id: string | null;
  session_binding_nonce: string | null;
}

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

export interface ChatStartResult extends TranscriptAuthority {
  kind?: "snapshot" | "unchanged" | "delta";
  file_length?: number;
  text: string;
  offset: number;
  offset_kind?: "bytes" | "items";
  start_offset?: number;
  pending?: boolean;
  /** The provider named no transcript source for this session, so nothing is
   *  loading one; `pending` here means unavailable, not "not yet". */
  source_missing?: boolean;
  messages?: ChatMessage[];
  /** Read-only rows from earlier provider sessions, before the active session window. */
  archived_messages?: ChatMessage[];
  cursor?: string;
  subscription_id?: string | null;
}

export interface KnownChatWindow extends TranscriptAuthority {
  source_session_id: string;
  end_offset: number;
  file_length: number;
  cursor: string;
}

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

export interface ChatBackfillResult extends TranscriptAuthority {
  text: string;
  offset_kind?: "bytes" | "items";
  start_offset: number;
  end_offset: number;
  messages?: ChatMessage[];
}

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

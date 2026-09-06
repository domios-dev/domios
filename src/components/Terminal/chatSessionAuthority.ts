import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";

export interface TranscriptAuthority {
  source_session_id: string | null;
  session_binding_nonce: string | null;
}

export type ChatAttachmentState =
  | "idle"
  | "attaching"
  | "awaiting_session"
  | "awaiting_store"
  | "attached"
  | "failed";

/** Why the host cannot show a transcript for a session it is bound to. Both are
 *  reported states, not waits: an observed delta is what clears either. */
export type ChatUnavailableReason = "no_transcript_source" | "authority_mismatch";

export interface ChatStartResult extends TranscriptAuthority {
  text: string;
  offset: number;
  start_offset?: number;
  pending?: boolean;
  /** The provider named no transcript source for this session, so nothing is
   *  loading one; `pending` here means unavailable, not "not yet". */
  source_missing?: boolean;
  messages?: ChatMessage[];
  cursor?: string;
  subscription_id?: string | null;
}

export interface ChatBackfillResult extends TranscriptAuthority {
  text: string;
  start_offset: number;
  end_offset: number;
  messages?: ChatMessage[];
}

export interface ChatMsgEvent extends TranscriptAuthority {
  tab_id: string;
  messages: ChatMessage[];
  cursor?: string;
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
  return incoming.source_session_id === tabSessionId
    && (incoming.session_binding_nonce ?? null) === tabSessionBindingNonce;
}

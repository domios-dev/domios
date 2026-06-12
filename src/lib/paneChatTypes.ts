// Types mirror Rust `agent/chat_types.rs` for UI chat payloads.

export interface MessageBlock {
  kind: string;
  data: Record<string, unknown>;
}

export interface RichMessage {
  role: string;
  blocks: MessageBlock[];
  ts?: number;
}

export interface ChatStartResult {
  snapshot: RichMessage[];
  offset: number;
}

export interface ChatBackfillResult {
  messages: RichMessage[];
  start_offset: number;
  end_offset: number;
}

export interface ChatDeltaEvent {
  pane_id: string;
  messages: RichMessage[];
}

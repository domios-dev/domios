// Shared types matching Rust's RichMessage / MessageBlock / SessionDelta.
// Keep in sync with src-tauri/src/agent/chat_types.rs.

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

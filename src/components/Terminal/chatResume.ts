import type { NormalizedChatMessage } from "@codeterm/chat-core";

export interface ChatResumeDelta {
  messages: NormalizedChatMessage[];
  start_offset: number;
  offset: number;
  cursor: string;
  source_session_id: string;
  session_binding_nonce: string | null;
}

export type ChatResumeResult =
  | { kind: "resumed"; deltas: ChatResumeDelta[]; caught_up: boolean }
  | { kind: "reset_required"; reason: "epoch_changed" | "compacted" | "invalid_cursor" }
  | { kind: "unsupported"; source_kind: "bytes" | "revision" | "items" };

export function isByteResumeCursor(token: string | null): token is string {
  if (token == null) return false;
  try {
    const cursor = JSON.parse(token);
    return cursor.kind === "bytes" && typeof cursor.incarnation === "string"
      && Number.isSafeInteger(cursor.offset) && cursor.offset >= 0;
  } catch { return false; }
}

export async function resumeChatCursor(
  cursor: string,
  read: (cursor: string) => Promise<ChatResumeResult>,
  adopt: (delta: ChatResumeDelta) => void,
  isCurrent: () => boolean,
): Promise<"caught_up" | "attach" | "superseded"> {
  let next = cursor;
  let pages = 0;
  let bytes = 0;
  while (isCurrent()) {
    const result = await read(next);
    if (!isCurrent()) return "superseded";
    if (result.kind !== "resumed") return "attach";
    pages++;
    bytes += result.deltas.reduce((total, delta) => total + Math.max(0, delta.offset - delta.start_offset), 0);
    for (const delta of result.deltas) adopt(delta);
    if (result.caught_up) return "caught_up";
    if (pages >= 8 || bytes >= 8 * 1024 * 1024) return "attach";
    const continuation = result.deltas[result.deltas.length - 1]?.cursor;
    if (continuation == null || continuation === next) throw new Error("Chat resume did not advance its cursor");
    next = continuation;
  }
  return "superseded";
}

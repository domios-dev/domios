import type { NormalizedChatMessage } from "@codeterm/chat-core";

export type ChatResumeDelta = Omit<import("../../../gen/ResumeDelta").ResumeDelta, "messages"> & {messages: NormalizedChatMessage[]};

export type ChatResumeResult = Exclude<import("../../../gen/ChatResumeResult").ChatResumeResult, {kind: "resumed"}> | (Omit<Extract<import("../../../gen/ChatResumeResult").ChatResumeResult, {kind: "resumed"}>, "deltas"> & {deltas: ChatResumeDelta[]});

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
  adopt: (delta: ChatResumeDelta) => boolean | void,
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
    for (const delta of result.deltas) {
      if (adopt(delta) === false) return "attach";
    }
    if (result.caught_up) return "caught_up";
    if (pages >= 8 || bytes >= 8 * 1024 * 1024) return "attach";
    const continuation = result.deltas[result.deltas.length - 1]?.cursor;
    if (continuation == null || continuation === next) throw new Error("Chat resume did not advance its cursor");
    next = continuation;
  }
  return "superseded";
}

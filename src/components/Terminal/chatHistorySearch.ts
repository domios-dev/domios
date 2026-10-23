import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { olderChatAction, type OlderChatAction } from "./chatWindow";

const CHAT_HISTORY_PAGE_BYTES = 1024 * 1024;
const CHAT_HISTORY_SCAN_BYTES = 2 * CHAT_HISTORY_PAGE_BYTES;

export interface OlderChatPage {
  cursor: number;
  consumedBytes: number;
  rows: number;
  messages?: NormalizedChatMessage[];
}

export async function searchOlderChatHistory(
  beforeOffset: number,
  loadPage: (cursor: number, maxBytes: number) => Promise<OlderChatPage | null>,
  byteBudget = CHAT_HISTORY_SCAN_BYTES,
  enough: (page: OlderChatPage) => boolean = page => page.rows > 0,
): Promise<OlderChatAction> {
  let action: OlderChatAction = { kind: "backfill", beforeOffset };
  let pages = 0;
  let consumedBytes = 0;
  let foundRows = false;
  while (action.kind === "backfill") {
    const previousCursor = action.beforeOffset;
    const page = await loadPage(previousCursor, Math.min(CHAT_HISTORY_PAGE_BYTES, byteBudget - consumedBytes));
    if (!page) return { kind: "none" };
    foundRows ||= page.rows > 0;
    pages++;
    consumedBytes += page.consumedBytes;
    action = olderChatAction([], null, true, page.cursor, false, {
      previousCursor, pages, consumedBytes, byteBudget, rows: enough(page) || (page.cursor === 0 && foundRows) ? 1 : 0,
    });
  }
  return action;
}

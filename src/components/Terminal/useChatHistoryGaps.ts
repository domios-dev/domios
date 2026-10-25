import { useCallback, useRef, useState } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { conversationEntries, CHAT_PAGE_SIZE, earlierMessageLabel } from "./chatPagingModel";
import type { EnrichedChatEntry } from "./chatEnrichment";

interface GapPage {
  rows: NormalizedChatMessage[];
  entries: EnrichedChatEntry[];
  revealed: number;
  next: NormalizedChatMessage | null;
}

const EMPTY_PAGES = new Map<string, GapPage>();

export function useChatHistoryGaps(windowKey: string,
  loadPage?: (message: NormalizedChatMessage) => Promise<NormalizedChatMessage[] | null>,
) {
  const [snapshot, setSnapshot] = useState({ key: windowKey, pages: new Map<string, GapPage>() });
  const pages = snapshot.key === windowKey ? snapshot.pages : EMPTY_PAGES;
  const inFlight = useRef(new Set<string>());
  const currentKey = useRef(windowKey);
  currentKey.current = windowKey;
  const label = useCallback((message: NormalizedChatMessage) => {
    const page = pages.get(message.id);
    return earlierMessageLabel(page ? page.entries.length - page.revealed : 0, page ? page.next != null : true);
  }, [pages]);
  const project = useCallback((entries: EnrichedChatEntry[]) => entries.flatMap(entry => {
    const page = pages.get(entry.msg.id);
    if (!page) return [entry];
    const visible = page.entries.slice(-page.revealed);
    return page.revealed < page.entries.length || page.next ? [entry, ...visible] : visible;
  }), [pages]);
  const load = async (message: NormalizedChatMessage): Promise<number> => {
    if (!loadPage || inFlight.current.has(message.id)) return 0;
    inFlight.current.add(message.id);
    try {
      const previous = pages.get(message.id);
      let page: GapPage = previous ?? { rows: [], entries: [], revealed: 0, next: message };
      const target = page.revealed + CHAT_PAGE_SIZE;
      let consumed = 0;
      for (let request = 0; page.entries.length < target && page.next && request < 8 && consumed < 2 * 1024 * 1024; request++) {
        const before = page.next.timelineMarker;
        const rows = await loadPage(page.next);
        if (currentKey.current !== windowKey || rows == null) return 0;
        const next = rows.find(row => row.timelineMarker?.kind === "history_gap") ?? null;
        const after = next?.timelineMarker;
        if (before?.kind === "history_gap" && after?.kind === "history_gap") {
          if (after.before_offset >= before.before_offset) throw new Error("Older history cursor did not advance");
          consumed += before.before_offset - after.before_offset;
        }
        const merged = [...rows.filter(row => row !== next), ...page.rows];
        page = { rows: merged, entries: conversationEntries(merged), revealed: page.revealed, next };
      }
      const revealed = Math.min(target, page.entries.length);
      setSnapshot(current => ({ key: windowKey, pages: new Map(current.key === windowKey ? current.pages : [])
        .set(message.id, { ...page, revealed }) }));
      return revealed - page.revealed;
    } finally {
      inFlight.current.delete(message.id);
    }
  };
  return { project, label, load };
}

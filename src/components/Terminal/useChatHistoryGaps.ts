import { useCallback, useMemo, useRef, useState } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { conversationEntries, CHAT_PAGE_SIZE, earlierMessageLabel } from "./chatPagingModel";
import type { EnrichedChatEntry } from "./chatEnrichment";

interface GapPage {
  rows: NormalizedChatMessage[];
  entries: EnrichedChatEntry[];
  cwd?: string | null;
  revealed: number;
  next: NormalizedChatMessage | null;
}

const EMPTY_PAGES = new Map<string, GapPage>();

export function useChatHistoryGaps(windowKey: string,
  loadPage?: (message: NormalizedChatMessage, signal: AbortSignal) => Promise<NormalizedChatMessage[] | null>,
  cwd?: string | null,
) {
  const [snapshot, setSnapshot] = useState({ key: windowKey, pages: new Map<string, GapPage>() });
  const pages = useMemo(() => {
    if (snapshot.key !== windowKey) return EMPTY_PAGES;
    if ([...snapshot.pages.values()].every(page => page.cwd === cwd)) return snapshot.pages;
    return new Map([...snapshot.pages].map(([id, page]) => [id, page.cwd === cwd
      ? page : { ...page, cwd, entries: conversationEntries(page.rows, undefined, cwd) }]));
  }, [snapshot, windowKey, cwd]);
  const inFlight = useRef(new Map<string, AbortSignal>());
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
  const load = async (message: NormalizedChatMessage, signal: AbortSignal): Promise<number> => {
    if (!loadPage || signal.aborted || inFlight.current.get(message.id) === signal) return 0;
    inFlight.current.set(message.id, signal);
    try {
      const previous = pages.get(message.id);
      let page: GapPage = previous ?? { rows: [], entries: [], revealed: 0, next: message };
      const target = page.revealed + CHAT_PAGE_SIZE;
      let consumed = 0;
      for (let request = 0; page.entries.length < target && page.next && request < 8 && consumed < 2 * 1024 * 1024; request++) {
        const before = page.next.timelineMarker;
        const rows = await loadPage(page.next, signal);
        if (signal.aborted || currentKey.current !== windowKey || rows == null) return 0;
        const next = rows.find(row => row.timelineMarker?.kind === "history_gap") ?? null;
        const after = next?.timelineMarker;
        if (before?.kind === "history_gap" && after?.kind === "history_gap") {
          if (after.before_offset >= before.before_offset) throw new Error("Older history cursor did not advance");
          consumed += before.before_offset - after.before_offset;
        }
        const merged = [...rows.filter(row => row !== next), ...page.rows];
        page = { rows: merged, cwd, entries: conversationEntries(merged, undefined, cwd), revealed: page.revealed, next };
      }
      const revealed = Math.min(target, page.entries.length);
      setSnapshot(current => ({ key: windowKey, pages: new Map(current.key === windowKey ? current.pages : [])
        .set(message.id, { ...page, revealed }) }));
      return revealed - page.revealed;
    } finally {
      if (inFlight.current.get(message.id) === signal) inFlight.current.delete(message.id);
    }
  };
  return { project, label, load };
}

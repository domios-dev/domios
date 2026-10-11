import { useCallback, useLayoutEffect, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { CHAT_ROW_ESTIMATE, chatRowOffsets, mountedChatWindow } from "./chatMountedWindow";

interface ChatWindowRow { msg: { id: string; sourceMessageId?: string } }

export function useMountedChatWindow({ rows, scrollRef, active, followTailRef, sessionKey }: {
  rows: readonly ChatWindowRow[];
  scrollRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  followTailRef: MutableRefObject<boolean>;
  sessionKey: string;
}) {
  const heights = useRef(new Map<string, number>());
  const keyRef = useRef(sessionKey);
  if (keyRef.current !== sessionKey) {
    keyRef.current = sessionKey;
    heights.current.clear();
  }
  const [viewport, setViewport] = useState({ top: 0, height: 0, revision: 0 });
  const [target, setTarget] = useState<string | null>(null);
  const topSpacerRef = useRef<HTMLDivElement>(null);
  const rowHeights = rows.map(({ msg }) => heights.current.get(msg.id) ?? CHAT_ROW_ESTIMATE);
  const targetIndex = target == null ? -1 : rows.findIndex(({ msg }) => msg.id === target);
  const range = mountedChatWindow({ heights: rowHeights, scrollTop: viewport.top, viewportHeight: viewport.height,
    active, pinned: followTailRef.current, targetIndex: targetIndex < 0 ? undefined : targetIndex });
  const live = useRef({ rows, rowHeights, range });
  live.current = { rows, rowHeights, range };

  const readViewport = useCallback(() => {
    const el = scrollRef.current;
    const spacer = topSpacerRef.current;
    if (!el || !spacer || el.clientHeight === 0) return;
    const origin = spacer.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    const top = el.scrollTop - origin;
    setViewport((previous) => previous.top === top && previous.height === el.clientHeight
      ? previous : { ...previous, top, height: el.clientHeight });
  }, [scrollRef]);

  const prepareMessage = useCallback((id: string) => {
    const { rows: current, range: mounted } = live.current;
    const index = current.findIndex(({ msg }) => msg.id === id || (msg.sourceMessageId ?? msg.id) === id);
    if (index < 0 || (index >= mounted.start && index < mounted.end)) return false;
    followTailRef.current = false;
    setTarget(current[index].msg.id);
    return true;
  }, [followTailRef]);

  useLayoutEffect(() => {
    if (!active) return;
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => { setTarget(null); readViewport(); };
    el.addEventListener("scroll", onScroll);
    readViewport();
    return () => el.removeEventListener("scroll", onScroll);
  }, [active, readViewport, scrollRef]);

  useLayoutEffect(() => {
    if (!active) return;
    const el = scrollRef.current;
    if (!el) return;
    const mounted = Array.from(el.querySelectorAll<HTMLElement>("[data-chat-window-row]"));
    const measure = () => {
      let changed = false;
      for (const row of mounted) {
        const id = row.dataset.chatWindowRow;
        const height = row.getBoundingClientRect().height;
        if (!id || height <= 0 || heights.current.get(id) === height) continue;
        heights.current.set(id, height);
        changed = true;
      }
      if (changed) setViewport((previous) => ({ ...previous, revision: previous.revision + 1 }));
      readViewport();
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(el);
    for (const row of mounted) observer?.observe(row);
    const ids = new Set(rows.map(({ msg }) => msg.id));
    for (const id of heights.current.keys()) if (!ids.has(id)) heights.current.delete(id);
    return () => observer?.disconnect();
  }, [active, rows, range.start, range.end, readViewport, scrollRef]);

  return { range, topSpacerRef, prepareMessage, revision: viewport.revision,
    offsets: chatRowOffsets(rowHeights) };
}

import { useCallback, useLayoutEffect, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { chatAnchorIdentities, chatRevealIndex, resolveChatAnchorIndex, type ChatContentAnchor } from "./chatAnchorModel";
import { getPersistedChatScrollState } from "./chatViewport";
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
  const persisted = getPersistedChatScrollState(sessionKey);
  const [viewport, setViewport] = useState({ key: sessionKey, top: persisted?.scrollTop ?? 0, height: 0, revision: 0 });
  const anchorRef = useRef<ChatContentAnchor | null>(persisted?.anchor ?? null);
  const keyRef = useRef(sessionKey);
  if (keyRef.current !== sessionKey) {
    keyRef.current = sessionKey;
    heights.current.clear();
    anchorRef.current = persisted?.anchor ?? null;
  }
  const [target, setTarget] = useState<{ key: string; id: string } | null>(null);
  const topSpacerRef = useRef<HTMLDivElement>(null);
  const rowHeights = rows.map(({ msg }) => heights.current.get(msg.id) ?? CHAT_ROW_ESTIMATE);
  const identities = chatAnchorIdentities(rows.map(({ msg }) => msg));
  const offsets = chatRowOffsets(rowHeights);
  const anchorIndex = anchorRef.current == null ? -1 : resolveChatAnchorIndex(identities, anchorRef.current);
  const anchoredTop = anchorIndex < 0 ? viewport.key === sessionKey ? viewport.top : persisted?.scrollTop ?? 0 : offsets[anchorIndex] - anchorRef.current!.offset;
  const targetIndex = target?.key !== sessionKey ? -1 : rows.findIndex(({ msg }) => msg.id === target.id);
  const range = mountedChatWindow({ heights: rowHeights, scrollTop: anchoredTop, viewportHeight: viewport.height,
    active, pinned: viewport.height === 0 ? persisted?.mode !== "detached" && followTailRef.current : followTailRef.current,
    targetIndex: targetIndex >= 0 ? targetIndex : viewport.height === 0 && anchorIndex >= 0 ? anchorIndex : undefined });
  const live = useRef({ rows, identities, rowHeights, range });
  live.current = { rows, identities, rowHeights, range };

  const readViewport = useCallback(() => {
    const el = scrollRef.current;
    const spacer = topSpacerRef.current;
    if (!el || !spacer || el.clientHeight === 0) return;
    const origin = spacer.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    const top = el.scrollTop - origin;
    const wrapper = Array.from(el.querySelectorAll<HTMLElement>("[data-chat-window-row]"))
      .find((row) => row.offsetTop >= el.scrollTop && row.offsetTop < el.scrollTop + el.clientHeight);
    const identity = wrapper && live.current.identities.find((row) => row.id === wrapper.dataset.chatWindowRow);
    anchorRef.current = identity && wrapper ? { ...identity, offset: wrapper.offsetTop - el.scrollTop } : null;
    setViewport((previous) => previous.key === sessionKey && previous.top === top && previous.height === el.clientHeight
      ? previous : { ...previous, key: sessionKey, top, height: el.clientHeight });
  }, [scrollRef, sessionKey]);

  const prepareMessage = useCallback((id: string) => {
    const { rows: current, range: mounted } = live.current;
    const index = chatRevealIndex(current.map(({ msg }) => msg), id);
    if (index < 0 || (index >= mounted.start && index < mounted.end)) return false;
    followTailRef.current = false;
    setTarget({ key: sessionKey, id: current[index].msg.id });
    return true;
  }, [followTailRef, sessionKey]);

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

  return { range, topSpacerRef, prepareMessage, syncViewport: readViewport, revision: viewport.revision,
    offsets, identities };
}

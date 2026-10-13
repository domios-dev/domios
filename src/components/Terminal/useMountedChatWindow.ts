import { useCallback, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject, type RefObject } from "react";
import type { ChatContentAnchor } from "./chatAnchorModel";
import { getPersistedChatScrollState } from "./chatViewport";
import { mountedChatWindowFromGeometry, sameMountedChatWindow } from "./chatMountedWindow";
import { indexChatTranscript, indexedChatAnchor, type ChatTranscriptIndex } from "./chatMountedTranscript";

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
  const viewport = useRef({ top: persisted?.scrollTop ?? 0, height: 0 });
  const anchorRef = useRef<ChatContentAnchor | null>(persisted?.anchor ?? null);
  const keyRef = useRef(sessionKey);
  const transcriptRef = useRef<ChatTranscriptIndex | undefined>(undefined);
  const targetRef = useRef<string | null>(null);
  const [revision, setRevision] = useState(0);
  if (keyRef.current !== sessionKey) {
    keyRef.current = sessionKey;
    heights.current.clear();
    transcriptRef.current = undefined;
    anchorRef.current = persisted?.anchor ?? null;
    viewport.current = { top: persisted?.scrollTop ?? 0, height: 0 };
    targetRef.current = null;
  }
  const transcript = useMemo(() => {
    const next = indexChatTranscript(rows.map(({ msg }) => msg), heights.current, transcriptRef.current);
    transcriptRef.current = next;
    for (const id of heights.current.keys()) if (!next.byId.has(id)) heights.current.delete(id);
    return next;
  }, [rows, sessionKey]);
  const topSpacerRef = useRef<HTMLDivElement>(null);
  const anchorIndex = anchorRef.current == null ? -1 : indexedChatAnchor(transcript, anchorRef.current);
  const anchoredTop = anchorIndex < 0 ? viewport.current.top : transcript.geometry.offset(anchorIndex) - anchorRef.current!.offset;
  const targetIndex = targetRef.current == null ? -1 : transcript.byId.get(targetRef.current) ?? -1;
  const range = mountedChatWindowFromGeometry({ geometry: transcript.geometry, scrollTop: anchoredTop, viewportHeight: viewport.current.height,
    active, pinned: viewport.current.height === 0 ? persisted?.mode !== "detached" && followTailRef.current : followTailRef.current,
    targetIndex: targetIndex >= 0 ? targetIndex : viewport.current.height === 0 && anchorIndex >= 0 ? anchorIndex : undefined });
  const live = useRef({ transcript, range, active });
  live.current = { transcript, range, active };

  const readViewport = useCallback((captureAnchor = true) => {
    const el = scrollRef.current;
    const spacer = topSpacerRef.current;
    if (!el || !spacer || el.clientHeight === 0) return;
    const origin = spacer.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    const top = el.scrollTop - origin;
    const current = live.current;
    if (captureAnchor) {
      const wrapper = Array.from(el.querySelectorAll<HTMLElement>("[data-chat-window-row]"))
        .find((row) => row.offsetTop >= el.scrollTop && row.offsetTop < el.scrollTop + el.clientHeight);
      const index = wrapper ? current.transcript.byId.get(wrapper.dataset.chatWindowRow!) : undefined;
      const identity = index == null ? undefined : current.transcript.identities[index];
      anchorRef.current = identity && wrapper ? { ...identity, offset: wrapper.offsetTop - el.scrollTop } : null;
    }
    viewport.current = { top, height: el.clientHeight };
    const anchorIndex = anchorRef.current == null ? -1 : indexedChatAnchor(current.transcript, anchorRef.current);
    const anchoredTop = anchorIndex < 0 ? top : current.transcript.geometry.offset(anchorIndex) - anchorRef.current!.offset;
    const targetIndex = targetRef.current == null ? undefined : current.transcript.byId.get(targetRef.current);
    const next = mountedChatWindowFromGeometry({ geometry: current.transcript.geometry, scrollTop: anchoredTop,
      viewportHeight: el.clientHeight, active: current.active, pinned: followTailRef.current, targetIndex });
    if (!sameMountedChatWindow(current.range, next)) {
      live.current = { ...current, range: next };
      setRevision((previous) => previous + 1);
    }
  }, [scrollRef, followTailRef]);

  const prepareMessage = useCallback((id: string) => {
    const { transcript: current, range: mounted } = live.current;
    const index = current.bySource.get(id) ?? current.byId.get(id) ?? -1;
    if (index < 0 || (index >= mounted.start && index < mounted.end)) return false;
    followTailRef.current = false;
    targetRef.current = current.messages[index].id;
    setRevision((previous) => previous + 1);
    return true;
  }, [followTailRef]);

  useLayoutEffect(() => {
    if (!active) return;
    const el = scrollRef.current;
    if (!el) return;
    let frame: number | null = null;
    const onScroll = () => {
      if (frame != null) return;
      frame = window.requestAnimationFrame(() => {
        frame = null;
        targetRef.current = null;
        readViewport();
      });
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    readViewport();
    return () => {
      el.removeEventListener("scroll", onScroll);
      if (frame != null) window.cancelAnimationFrame(frame);
    };
  }, [active, sessionKey, readViewport, scrollRef]);

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
        const index = id == null ? undefined : transcript.byId.get(id);
        if (id == null || index == null || height <= 0) continue;
        heights.current.set(id, height);
        changed = transcript.geometry.update(index, height) || changed;
      }
      if (changed) setRevision((previous) => previous + 1);
      readViewport(false);
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(el);
    for (const row of mounted) observer?.observe(row);
    return () => observer?.disconnect();
  }, [active, transcript, range.start, range.end, readViewport, scrollRef]);

  return { range, topSpacerRef, prepareMessage, syncViewport: readViewport, revision, identities: transcript.identities };
}

import { DEV_BUILD } from "../../lib/devBuild";
import { chatTraceActive, chatTraceEdge, registerChatTraceProbe, traceIdentity } from "./chatGestureTraceRuntime";
import { useCallback, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject, type RefObject } from "react";
import type { ChatContentAnchor } from "./chatAnchorModel";
import { getPersistedChatScrollState } from "./chatViewport";
import { chatScrollBookmark } from "./chatScrollMemory";
import { mountedChatWindowFromGeometry, sameMountedChatWindow } from "./chatMountedWindow";
import { indexChatTranscript, indexedChatAnchor, type ChatTranscriptIndex } from "./chatMountedTranscript";
import { chatAnchorShift, chatShiftAnchorAt, chatSpacerAfterLayout, chatSpacerFold, chatSpacerHeight, chatSpacerTranslation, chatLayoutViewportTop, shouldAbsorbChatShift,
  type ChatShiftAnchor } from "./chatSpacerDrift";

function supportsNativeScrollAnchoring(): boolean {
  return typeof CSS !== "undefined" && typeof CSS.supports === "function" && CSS.supports("overflow-anchor", "auto");
}

function mountedRowTops(el: HTMLElement): { rows: ChatShiftAnchor[]; heights: number[] } {
  const origin = el.getBoundingClientRect().top - el.scrollTop;
  const rows: ChatShiftAnchor[] = [];
  const heights: number[] = [];
  for (const row of Array.from(el.querySelectorAll<HTMLElement>("[data-chat-window-row]"))) {
    const box = row.getBoundingClientRect();
    rows.push({ id: row.dataset.chatWindowRow!, top: box.top - origin });
    heights.push(box.height);
  }
  return { rows, heights };
}

interface ChatWindowRow { msg: { id: string; sourceMessageId?: string } }

function translateRows(rows: HTMLElement | null, translation: number): void {
  if (rows) rows.style.transform = translation === 0 ? "" : `translateY(${translation}px)`;
}

export function useMountedChatWindow({ rows, scrollRef, active, followTailRef, sessionKey, userScrollingRef,
  nativeAnchoring = supportsNativeScrollAnchoring() }: {
  rows: readonly ChatWindowRow[];
  scrollRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  followTailRef: MutableRefObject<boolean>;
  sessionKey: string;
  userScrollingRef?: MutableRefObject<boolean>;
  nativeAnchoring?: boolean;
}) {
  const heights = useRef(new Map<string, number>());
  const persisted = getPersistedChatScrollState(sessionKey);
  const bookmark = chatScrollBookmark(persisted);
  const activeRef = useRef(false);
  const viewport = useRef({ top: persisted?.scrollTop ?? 0, height: 0 });
  const anchorRef = useRef<ChatContentAnchor | null>(bookmark.mode === "detached" ? bookmark.anchor : null);
  const keyRef = useRef(sessionKey);
  const transcriptRef = useRef<ChatTranscriptIndex | undefined>(undefined);
  const targetRef = useRef<string | null>(null);
  const [revision, setRevision] = useState(0);
  const driftRef = useRef(0);
  const shiftAnchorRef = useRef<ChatShiftAnchor | null>(null);
  if (keyRef.current !== sessionKey) {
    keyRef.current = sessionKey;
    driftRef.current = 0;
    shiftAnchorRef.current = null;
    heights.current.clear();
    transcriptRef.current = undefined;
    anchorRef.current = bookmark.mode === "detached" ? bookmark.anchor : null;
    viewport.current = { top: persisted?.scrollTop ?? 0, height: 0 };
    targetRef.current = null;
    followTailRef.current = bookmark.mode === "pinned";
  }
  if (active && !activeRef.current) {
    anchorRef.current = bookmark.mode === "detached" ? bookmark.anchor : null;
    viewport.current = { top: 0, height: 0 };
    followTailRef.current = bookmark.mode === "pinned";
  }
  activeRef.current = active;
  const transcript = useMemo(() => {
    const next = indexChatTranscript(rows.map(({ msg }) => msg), heights.current, transcriptRef.current);
    transcriptRef.current = next;
    for (const id of heights.current.keys()) if (!next.byId.has(id)) heights.current.delete(id);
    return next;
  }, [rows, sessionKey]);
  const topSpacerRef = useRef<HTMLDivElement>(null);
  const rowsRef = useRef<HTMLDivElement>(null);
  const anchorIndex = anchorRef.current == null ? -1 : indexedChatAnchor(transcript, anchorRef.current);
  const anchoredTop = anchorIndex < 0 ? viewport.current.top : transcript.geometry.offset(anchorIndex) - anchorRef.current!.offset;
  const targetIndex = targetRef.current == null ? -1 : transcript.byId.get(targetRef.current) ?? -1;
  const range = mountedChatWindowFromGeometry({ geometry: transcript.geometry, scrollTop: anchoredTop, viewportHeight: viewport.current.height,
    active, pinned: followTailRef.current,
    targetIndex: targetIndex >= 0 ? targetIndex : viewport.current.height === 0 && anchorIndex >= 0 ? anchorIndex : undefined });
  const live = useRef({ transcript, range, active });
  live.current = { transcript, range, active };

  useLayoutEffect(() => {
    if (!DEV_BUILD || !active) return;
    return registerChatTraceProbe(sessionKey, () => {
      const anchor = shiftAnchorRef.current;
      const row = anchor && Array.from(scrollRef.current?.querySelectorAll<HTMLElement>("[data-chat-window-row]") ?? [])
        .find(row => row.dataset.chatWindowRow === anchor.id);
      return [driftRef.current, chatSpacerTranslation(live.current.range.top, driftRef.current), live.current.range.start,
        live.current.range.end, traceIdentity(anchor?.id), row?.getBoundingClientRect().top ?? 0];
    });
  }, [active, sessionKey, scrollRef]);

  const captureShiftAnchor = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const { rows: tops, heights } = mountedRowTops(el);
    shiftAnchorRef.current = chatShiftAnchorAt(tops, heights, el.scrollTop);
  }, [scrollRef]);

  // Runs after every layout change of the mounted rows, before paint, so a shift above the view never reaches the screen.
  const absorbShift = useCallback(() => {
    const el = scrollRef.current;
    const spacer = topSpacerRef.current;
    if (!el || !spacer || el.clientHeight === 0) return;
    const anchor = shiftAnchorRef.current;
    const base = live.current.range.top;
    // React skips an unchanged style prop, so a height written here earlier would otherwise survive and read as a shift.
    spacer.style.height = `${chatSpacerHeight(base, driftRef.current)}px`;
    translateRows(rowsRef.current, chatSpacerTranslation(base, driftRef.current));
    if (followTailRef.current) {
      if (DEV_BUILD && chatTraceActive(keyRef.current)) chatTraceEdge(keyRef.current, "absorb", [chatAnchorShift(anchor,
        anchor ? mountedRowTops(el).rows.find(row => row.id === anchor.id)?.top ?? null : null), 0, driftRef.current]);
      driftRef.current = 0;
    }
    else {
      const absorb = anchor != null
        && shouldAbsorbChatShift({ nativeAnchoring, pinned: false, userScrolling: userScrollingRef?.current ?? false });
      const top = absorb ? mountedRowTops(el).rows.find((row) => row.id === anchor.id)?.top ?? null : null;
      if (DEV_BUILD && chatTraceActive(keyRef.current)) {
        const measuredTop = !absorb && anchor ? mountedRowTops(el).rows.find(row => row.id === anchor.id)?.top ?? null : top;
        chatTraceEdge(keyRef.current, "absorb", [chatAnchorShift(anchor, measuredTop), Number(absorb), driftRef.current]);
      }
      const next = chatSpacerAfterLayout({ base, drift: driftRef.current, shift: chatAnchorShift(anchor, top), absorb,
        viewportHeight: el.clientHeight, scrollTop: el.scrollTop });
      driftRef.current = next.drift;
    }
    const height = `${chatSpacerHeight(base, driftRef.current)}px`;
    if (spacer.style.height !== height) spacer.style.height = height;
    translateRows(rowsRef.current, chatSpacerTranslation(base, driftRef.current));
    captureShiftAnchor();
  }, [captureShiftAnchor, followTailRef, nativeAnchoring, scrollRef, userScrollingRef]);

  const foldDrift = useCallback(() => {
    const spacer = topSpacerRef.current;
    const fold = chatSpacerFold(live.current.range.top, driftRef.current);
    driftRef.current = fold.drift;
    if (spacer) spacer.style.height = `${chatSpacerHeight(live.current.range.top, 0)}px`;
    translateRows(rowsRef.current, 0);
    return fold.scrollBy;
  }, []);

  const readViewport = useCallback((captureAnchor = true) => {
    const el = scrollRef.current;
    const spacer = topSpacerRef.current;
    if (!el || !spacer || el.clientHeight === 0) return;
    captureShiftAnchor();
    const origin = spacer.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop
      + chatSpacerHeight(live.current.range.top, driftRef.current) - live.current.range.top;
    const top = chatLayoutViewportTop(el.scrollTop, origin, chatSpacerTranslation(live.current.range.top, driftRef.current));
    const current = live.current;
    if (!current.active) return;
    if (captureAnchor) {
      const measured = mountedRowTops(el);
      const wrapper = chatShiftAnchorAt(measured.rows, measured.heights, el.scrollTop);
      const index = wrapper ? current.transcript.byId.get(wrapper.id) : undefined;
      const identity = index == null ? undefined : current.transcript.identities[index];
      anchorRef.current = identity && wrapper ? { ...identity, offset: wrapper.top - el.scrollTop } : null;
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
  }, [scrollRef, followTailRef, captureShiftAnchor]);

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
    readViewport(false);
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
    // A hidden scroller reports zero for every row; a laid-out row of zero height is a real measurement.
    const measure = () => {
      if (el.clientHeight === 0) return;
      absorbShift();
      let changed = false;
      const deltas: number[] = [];
      for (const row of mounted) {
        const id = row.dataset.chatWindowRow;
        const height = row.getBoundingClientRect().height;
        const index = id == null ? undefined : transcript.byId.get(id);
        if (id == null || index == null) continue;
        if (DEV_BUILD && chatTraceActive(keyRef.current) && deltas.length < 32) {
          const delta = height - (transcript.geometry.offset(index + 1) - transcript.geometry.offset(index));
          if (delta !== 0) deltas.push(traceIdentity(id), delta);
        }
        heights.current.set(id, height);
        changed = transcript.geometry.update(index, height) || changed;
      }
      if (DEV_BUILD && chatTraceActive(keyRef.current)) chatTraceEdge(keyRef.current, "resize", [mounted.length, ...deltas]);
      if (changed) setRevision((previous) => previous + 1);
      readViewport(false);
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(el);
    for (const row of mounted) observer?.observe(row);
    return () => observer?.disconnect();
  }, [active, transcript, range.start, range.end, readViewport, scrollRef, absorbShift]);

  useLayoutEffect(absorbShift);

  return { range, topSpacer: chatSpacerHeight(range.top, driftRef.current), topSpacerRef, rowsRef, foldDrift, prepareMessage,
    syncViewport: readViewport, revision, identities: transcript.identities };
}

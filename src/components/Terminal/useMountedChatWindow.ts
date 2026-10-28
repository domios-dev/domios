import { DEV_BUILD } from "../../lib/devBuild";
import { chatTraceActive, chatTraceEdge, registerChatTraceProbe, traceIdentity } from "./chatGestureTraceRuntime";
import { useCallback, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject, type RefObject } from "react";
import type { ChatContentAnchor } from "./chatAnchorModel";
import { getPersistedChatScrollState } from "./chatViewport";
import { initialChatScrollAnchor } from "./chatScrollMemory";
import { mountedChatWindowFromGeometry, sameMountedChatWindow } from "./chatMountedWindow";
import { indexChatTranscript, indexedChatAnchor, type ChatTranscriptIndex } from "./chatMountedTranscript";
import { chatAnchorShift, chatShiftAnchorAt, chatSpacerAfterLayout, chatSpacerFold, chatSpacerHeight, chatSpacerTranslation, chatLayoutViewportTop, shouldAbsorbChatShift,
  unrecordedRowsAbove, type ChatShiftAnchor } from "./chatSpacerDrift";

function mountedRowTops(el: HTMLElement): { rows: ChatShiftAnchor[]; heights: number[]; elements: HTMLElement[] } {
  const origin = el.getBoundingClientRect().top - el.scrollTop;
  const rows: ChatShiftAnchor[] = [];
  const heights: number[] = [];
  const elements = Array.from(el.querySelectorAll<HTMLElement>("[data-chat-window-row]"));
  for (const row of elements) {
    const box = row.getBoundingClientRect();
    const body = row.querySelector<HTMLElement>("[data-message-id]");
    rows.push({ id: row.dataset.chatWindowRow!, top: box.top - origin, contentTop: body ? body.getBoundingClientRect().top - origin : undefined });
    heights.push(box.height);
  }
  return { rows, heights, elements };
}

interface ChatWindowRow { msg: { id: string; sourceMessageId?: string } }

function translateRows(rows: HTMLElement | null, translation: number): void {
  const transform = translation === 0 ? "" : `translateY(${translation}px)`;
  if (rows && rows.style.transform !== transform) rows.style.transform = transform;
}

export function useMountedChatWindow({ rows, scrollRef, active, followTailRef, sessionKey, userScrollingRef }: {
  rows: readonly ChatWindowRow[];
  scrollRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  followTailRef: MutableRefObject<boolean>;
  sessionKey: string;
  userScrollingRef?: MutableRefObject<boolean>;
}) {
  const heights = useRef(new Map<string, number>());
  const persisted = getPersistedChatScrollState(sessionKey);
  const initial = initialChatScrollAnchor(persisted);
  const activeRef = useRef(false);
  const viewport = useRef({ top: initial.top, height: 0 });
  const anchorRef = useRef<ChatContentAnchor | null>(initial.anchor);
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
    anchorRef.current = initial.anchor;
    viewport.current = { top: initial.top, height: 0 };
    targetRef.current = null;
    followTailRef.current = initial.pinned;
  }
  if (active && !activeRef.current) {
    anchorRef.current = initial.anchor;
    viewport.current = { top: initial.top, height: 0 };
    followTailRef.current = initial.pinned;
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
  const anchorIndex = followTailRef.current || anchorRef.current == null ? -1 : indexedChatAnchor(transcript, anchorRef.current);
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
        live.current.range.end, row ? traceIdentity(anchor?.id) : 0, row?.getBoundingClientRect().top ?? 0];
    });
  }, [active, sessionKey, scrollRef]);

  const captureShiftAnchor = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const { rows: tops, heights: measured, elements } = mountedRowTops(el);
    const next = chatShiftAnchorAt(tops, measured, el.scrollTop);
    // A resize above the anchor that its notification has not reported yet is absorbed against the old anchor first;
    // taking the new anchor now would hide that shift for good. The rows were just measured, so this reads nothing more.
    // Up to the previous anchor: a row that grew above it may now reach into the view and would be the new anchor itself.
    const previous = shiftAnchorRef.current ? tops.findIndex((row) => row.id === shiftAnchorRef.current!.id) : -1;
    const pending = unrecordedRowsAbove(tops, measured, Math.max(previous, next ? tops.indexOf(next) : -1), heights.current);
    if (pending.length > 0) {
      applyHeightsRef.current(pending.map((index) => [elements[index], measured[index]] as const));
      return;
    }
    shiftAnchorRef.current = next;
  }, [scrollRef]);

  // Runs after every layout change of the mounted rows, before paint, so a shift above the view never reaches the screen.
  const absorbShift = useCallback(() => {
    const el = scrollRef.current;
    const spacer = topSpacerRef.current;
    if (!el || !spacer || el.clientHeight === 0) return;
    const anchor = shiftAnchorRef.current;
    const base = live.current.range.top;
    // React skips an unchanged style prop, so a height written here earlier would otherwise survive and read as a shift.
    const written = `${chatSpacerHeight(base, driftRef.current)}px`;
    if (spacer.style.height !== written) spacer.style.height = written;
    translateRows(rowsRef.current, chatSpacerTranslation(base, driftRef.current));
    if (followTailRef.current) {
      if (DEV_BUILD && chatTraceActive(keyRef.current)) chatTraceEdge(keyRef.current, "absorb", [chatAnchorShift(anchor,
        anchor ? mountedRowTops(el).rows.find(row => row.id === anchor.id) : null), 0, driftRef.current]);
      driftRef.current = 0;
    }
    else {
      const absorb = anchor != null
        && shouldAbsorbChatShift({ pinned: false, userScrolling: userScrollingRef?.current ?? false });
      const measured = absorb ? mountedRowTops(el).rows.find((row) => row.id === anchor.id) : null;
      if (DEV_BUILD && chatTraceActive(keyRef.current)) {
        const traced = !absorb && anchor ? mountedRowTops(el).rows.find(row => row.id === anchor.id) : measured;
        chatTraceEdge(keyRef.current, "absorb", [chatAnchorShift(anchor, traced), Number(absorb), driftRef.current]);
      }
      const next = chatSpacerAfterLayout({ base, drift: driftRef.current, shift: chatAnchorShift(anchor, measured), absorb,
        viewportHeight: el.clientHeight, scrollTop: el.scrollTop });
      driftRef.current = next.drift;
    }
    const height = `${chatSpacerHeight(base, driftRef.current)}px`;
    if (spacer.style.height !== height) spacer.style.height = height;
    translateRows(rowsRef.current, chatSpacerTranslation(base, driftRef.current));
    captureShiftAnchor();
  }, [captureShiftAnchor, followTailRef, scrollRef, userScrollingRef]);

  const foldDrift = useCallback(() => {
    const spacer = topSpacerRef.current;
    const fold = chatSpacerFold(live.current.range.top, driftRef.current);
    if (DEV_BUILD && fold.scrollBy !== 0) chatTraceEdge(keyRef.current, "fold", [fold.scrollBy]);
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

  // Records row heights, then repairs the layout once: every read happens before the spacer and transform writes.
  const applyHeights = useCallback((measured: ReadonlyArray<readonly [HTMLElement, number]>) => {
    const el = scrollRef.current;
    // A hidden scroller reports zero for every row; a laid-out row of zero height is a real measurement.
    if (!el || el.clientHeight === 0) return;
    const { transcript: current } = live.current;
    let changed = false;
    const deltas: number[] = [];
    for (const [row, height] of measured) {
      const id = row.dataset.chatWindowRow;
      const index = id == null ? undefined : current.byId.get(id);
      if (id == null || index == null) continue;
      if (DEV_BUILD && chatTraceActive(keyRef.current) && deltas.length < 32) {
        const delta = height - (current.geometry.offset(index + 1) - current.geometry.offset(index));
        if (delta !== 0) deltas.push(traceIdentity(id), delta);
      }
      heights.current.set(id, height);
      changed = current.geometry.update(index, height) || changed;
    }
    if (DEV_BUILD && chatTraceActive(keyRef.current)) chatTraceEdge(keyRef.current, "resize", [measured.length, ...deltas]);
    absorbShift();
    if (changed) setRevision((previous) => previous + 1);
    readViewport(false);
  }, [absorbShift, readViewport, scrollRef]);
  const applyHeightsRef = useRef(applyHeights);
  applyHeightsRef.current = applyHeights;

  // One observer per shown session reports the scroller, its content and the mounted rows; rows join as they mount.
  const observerRef = useRef<ResizeObserver | null>(null);
  const observedRows = useRef(new Set<HTMLElement>());
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!active || !el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      applyHeightsRef.current(entries.filter((entry) => entry.target instanceof HTMLElement && entry.target.dataset.chatWindowRow != null)
        .map((entry) => [entry.target as HTMLElement, entry.borderBoxSize?.[0]?.blockSize ?? entry.target.getBoundingClientRect().height] as const));
    });
    observer.observe(el);
    if (el.firstElementChild) observer.observe(el.firstElementChild);
    observerRef.current = observer;
    const rows = observedRows.current;
    return () => {
      observer.disconnect();
      observerRef.current = null;
      rows.clear();
    };
  }, [active, sessionKey, scrollRef]);

  useLayoutEffect(() => {
    if (!active) return;
    const el = scrollRef.current;
    if (!el) return;
    const observer = observerRef.current;
    const observed = observedRows.current;
    const mounted = Array.from(el.querySelectorAll<HTMLElement>("[data-chat-window-row]"));
    const current = new Set(mounted);
    for (const row of observed) if (!current.has(row)) { observer?.unobserve(row); observed.delete(row); }
    // The observer keeps observed rows current; a newly mounted row is measured now so its first paint is placed.
    const fresh = observer ? mounted.filter((row) => !observed.has(row)) : mounted;
    applyHeights(fresh.map((row) => [row, row.getBoundingClientRect().height] as const));
    for (const row of fresh) if (observer) { observer.observe(row); observed.add(row); }
  }, [active, transcript, range.start, range.end, applyHeights, scrollRef]);

  useLayoutEffect(absorbShift, [absorbShift, range.top]);

  return { range, topSpacer: chatSpacerHeight(range.top, driftRef.current), topSpacerRef, rowsRef, foldDrift, prepareMessage,
    syncViewport: readViewport, revision, identities: transcript.identities };
}

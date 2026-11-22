import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { TRANSCRIPT_PAGE_SIZE, type LiveTranscriptApi, type RoutedPage, type TranscriptHistory, type TranscriptOverview, type TranscriptReadiness } from "../../lib/transport/liveTranscript";
import { useAttachments } from "./useAttachments";
import type { InlineAttachment, InlineAttachmentSink } from "./useComposerCapsules";

export interface LiveTranscriptOpenOptions {
  autostart?: boolean;
}

export interface LiveTranscriptHostState {
  ownerId: string | null;
  locked: boolean;
  open: (tabId: string, options?: LiveTranscriptOpenOptions) => void;
}

export const LiveTranscriptContext = createContext<LiveTranscriptHostState | null>(null);
export const useLiveTranscriptHost = () => useContext(LiveTranscriptContext);
export const LIVE_TRANSCRIPT_FILES = "domios:live-transcript-files";
export type LiveTranscriptFiles =
  | { ownerId: string; items: InlineAttachment[]; accepted?: boolean }
  | { ownerId: string; blob: Blob; name: string; upload?: Promise<string | null | undefined> };

export function useLiveTranscriptAttachments(ownerId: string, onFeedback: (message: string) => void) {
  const container = useRef<HTMLDivElement>(null);
  const { inlineSinkRef, handlePasteEvent, webDropHandlers, webDragOver } = useAttachments({
    chatRootRef: container, enabled: true, onError: onFeedback,
  });
  useLayoutEffect(() => {
    const transfer = (detail: LiveTranscriptFiles) => window.dispatchEvent(new CustomEvent(LIVE_TRANSCRIPT_FILES, { detail }));
    const sink: InlineAttachmentSink = items => {
      const detail: LiveTranscriptFiles = { ownerId, items };
      transfer(detail);
      onFeedback(detail.accepted ? "Added to the owner’s composer." : "Attachments are unavailable for this tab.");
    };
    sink.upload = async (blob, name) => {
      const detail: LiveTranscriptFiles = { ownerId, blob, name };
      transfer(detail);
      if (!detail.upload) {
        onFeedback("Attachments are unavailable for this tab.");
        return null;
      }
      const path = await detail.upload;
      if (path) onFeedback("Added to the owner’s composer.");
      return path;
    };
    inlineSinkRef.current = sink;
    return () => { inlineSinkRef.current = null; };
  }, [inlineSinkRef, onFeedback, ownerId]);
  return { container, handlePasteEvent, webDropHandlers, webDragOver };
}

type HistoryView = { from: number; through: number; page: TranscriptHistory };
type LogView = { after: number; page: RoutedPage };
type PanelOptions = { api: LiveTranscriptApi; show: boolean; log: boolean; captureRevision: number; capturePhase: string };

export function useLiveTranscriptPanel({ api, show, log, captureRevision, capturePhase }: PanelOptions) {
  const { subscribeChanges } = api;
  const [overview, setOverview] = useState<TranscriptOverview>();
  const [readiness, setReadiness] = useState<TranscriptReadiness>();
  const [history, setHistory] = useState<HistoryView>();
  const [events, setEvents] = useState<LogView>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const historyPosition = useRef<number | null>(null);
  const logPosition = useRef(0);
  const historyLoaded = useRef<HistoryView | undefined>(undefined);
  const session = useRef<AbortController | null>(null);
  const pending = useRef(false);
  const refreshing = useRef<AbortController | null>(null);
  const mutation = useRef<AbortController | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const [canFollow, setCanFollow] = useState(false);

  const refresh = useLatestCallback(async () => {
    const lifetime = session.current;
    if (!show || !lifetime || lifetime.signal.aborted) return;
    if (refreshing.current) { pending.current = true; return; }
    const request = new AbortController();
    refreshing.current = request;
    const signal = AbortSignal.any([request.signal, lifetime.signal]);
    try {
      const next = await api.overview(signal);
      if (signal.aborted) return;
      const through = next.status.committedSeq;
      const from = historyPosition.current ?? Math.max(1, through - TRANSCRIPT_PAGE_SIZE + 1);
      const cached = historyLoaded.current;
      const readHistory = !log && (!cached || cached.from !== from || (cached.through !== through && cached.through < from + TRANSCRIPT_PAGE_SIZE - 1));
      const [page, routes] = await Promise.all([
        readHistory ? api.history(from, TRANSCRIPT_PAGE_SIZE, signal) : undefined,
        log ? api.events(logPosition.current, signal) : undefined,
      ]);
      if (signal.aborted) return;
      setOverview(next);
      if (page) {
        const view = { from, through, page };
        historyLoaded.current = view;
        setHistory(view);
      }
      if (routes) setEvents({ after: logPosition.current, page: routes });
      setError(undefined);
    } catch (failure) {
      if (!signal.aborted) setError(errorText(failure));
    } finally {
      if (refreshing.current === request) {
        refreshing.current = null;
        if (!signal.aborted) setLoaded(true);
        if (pending.current) { pending.current = false; void refresh(); }
      }
    }
  }, [api, show]);

  const refreshReadiness = useLatestCallback(async () => {
    const signal = session.current?.signal;
    if (!signal || signal.aborted) return;
    try {
      const next = await api.readiness(signal);
      if (!signal.aborted) setReadiness(next);
    } catch (failure) {
      if (!signal.aborted) setError(errorText(failure));
    }
  }, [api]);

  useEffect(() => {
    if (!show) return;
    const lifetime = new AbortController();
    session.current = lifetime;
    const unsubscribe = subscribeChanges?.(refresh);
    void refreshReadiness();
    return () => {
      lifetime.abort();
      unsubscribe?.();
      refreshing.current = null;
      pending.current = false;
      session.current = null;
    };
  }, [show, refresh, refreshReadiness, subscribeChanges]);

  useEffect(() => { void refresh(); }, [refresh, log, captureRevision, capturePhase]);

  useEffect(() => () => mutation.current?.abort(), []);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && follow.current && !log) el.scrollTop = el.scrollHeight;
  }, [history, log, show, captureRevision]);

  function onScroll() {
    const el = scrollRef.current;
    if (!el || log) return;
    const following = el.scrollHeight - el.clientHeight - el.scrollTop < 24;
    if (following === follow.current) return;
    follow.current = following;
    if (following) {
      if (!canFollow) {
        historyPosition.current = null;
        void refresh();
      } else {
        follow.current = false;
      }
    } else {
      historyPosition.current = historyLoaded.current?.from ?? null;
    }
  }

  function moveHistory(from: number | null) {
    historyPosition.current = from;
    follow.current = from === null;
    setCanFollow(from !== null);
    if (scrollRef.current && from !== null) scrollRef.current.scrollTop = 0;
    refreshing.current?.abort();
    void refresh();
  }

  function moveLog(after: number) {
    logPosition.current = after;
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    refreshing.current?.abort();
    void refresh();
  }

  async function mutate(action: (signal: AbortSignal) => Promise<unknown>) {
    if (mutation.current) return false;
    const request = new AbortController();
    mutation.current = request;
    setBusy(true);
    try {
      await action(request.signal);
      if (request.signal.aborted) return false;
      void refresh();
      return true;
    } catch (failure) {
      if (!request.signal.aborted) setError(errorText(failure));
      return false;
    } finally {
      mutation.current = null;
      if (!request.signal.aborted) setBusy(false);
    }
  }

  return { overview, readiness, history, events, error, loaded, busy, refresh, refreshReadiness, mutate,
    scrollRef, onScroll, moveHistory, moveLog };
}

export function errorText(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

type Bounds = { left: number; top: number; width: number; height: number };
const MARGIN = 8;

function fit(bounds: Bounds): Bounds {
  const availableWidth = window.innerWidth - MARGIN * 2;
  const availableHeight = window.innerHeight - MARGIN * 2;
  const width = Math.max(Math.min(280, availableWidth), Math.min(bounds.width, availableWidth));
  const height = Math.max(Math.min(220, availableHeight), Math.min(bounds.height, availableHeight));
  return {
    width, height,
    left: Math.max(MARGIN, Math.min(bounds.left, window.innerWidth - width - MARGIN)),
    top: Math.max(MARGIN, Math.min(bounds.top, window.innerHeight - height - MARGIN)),
  };
}

export function useLiveTranscriptGeometry() {
  const [bounds, setBounds] = useState(() => fit({ left: window.innerWidth - 460, top: 72, width: 420, height: 440 }));
  const gesture = useRef<{ id: number; x: number; y: number; bounds: Bounds } | null>(null);
  useEffect(() => {
    const resize = () => setBounds(fit);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  function handle(mode: "move" | "size") {
    function update(start: Bounds, x: number, y: number) {
      setBounds(fit(mode === "move" ? { ...start, left: start.left + x, top: start.top + y }
        : { ...start, width: start.width + x, height: start.height + y }));
    }
    return {
      onPointerDown(event: PointerEvent<HTMLButtonElement>) {
        if (event.button !== 0) return;
        gesture.current = { id: event.pointerId, x: event.clientX, y: event.clientY, bounds };
        event.currentTarget.setPointerCapture(event.pointerId);
      },
      onPointerMove(event: PointerEvent<HTMLButtonElement>) {
        const start = gesture.current;
        if (start?.id === event.pointerId) update(start.bounds, event.clientX - start.x, event.clientY - start.y);
      },
      onLostPointerCapture() { gesture.current = null; },
      onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
        const x = event.code === "ArrowLeft" ? -16 : event.code === "ArrowRight" ? 16 : 0;
        const y = event.code === "ArrowUp" ? -16 : event.code === "ArrowDown" ? 16 : 0;
        if (!x && !y) return;
        event.preventDefault();
        event.stopPropagation();
        update(bounds, x, y);
      },
    };
  }
  return { bounds, handle };
}

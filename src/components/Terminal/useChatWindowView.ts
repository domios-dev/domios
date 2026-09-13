import { type MutableRefObject, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  lastSessionResetIndex,
  matchedOptimisticEchoes,
  nextOptimisticOrder,
  projectContextEpochs,
  projectTranscript,
  reduceToolEvents,
  stabilizeTimelineItems,
  transcriptBackfillCursor,
  transcriptRows,
  type NormalizedChatMessage as ChatMessage,
  type OptimisticEcho,
  type TimelineItem,
  type TranscriptState,
} from "@codeterm/chat-core";
import { enrichTimelineItems, type EnrichmentCache } from "./chatEnrichment";
import {
  chatWindowAnchorIdentity,
  chatWindowProjection,
  chatWindowRange,
  rekeyedChatWindowAnchor,
  slideChatWindowOlder,
  timelineItemKey,
  type ChatWindowAnchorIdentity,
} from "./chatWindow";
import { setTranscriptAnchor } from "./transcriptStore";

const MAX_RETAINED_OPTIMISTIC_MESSAGES = 16;
const MS_EPOCH_FLOOR = 1e12;

export function timestampMs(ts: string | undefined): number | null {
  if (!ts) return null;
  const numeric = Number(ts);
  if (Number.isFinite(numeric)) {
    if (numeric <= 0) return null;
    return numeric < MS_EPOCH_FLOOR ? numeric * 1000 : numeric;
  }
  const parsed = Date.parse(ts);
  return Number.isNaN(parsed) ? null : parsed;
}

function toEchoes(msgs: readonly ChatMessage[]): OptimisticEcho[] {
  return msgs.map((m) => ({
    id: m.id,
    content: m.content,
    sentAtMs: Number(m.timestamp) || 0,
    clientOrder: m.clientOrder,
    correlationId: m.correlationId,
  }));
}

/** Viewport projection over the committed transcript: window anchor, enriched
 *  rows and the optimistic tail. It reads the transcript and never stores one. */
export function useChatWindowView({
  windowKey,
  transcript,
  anchorKey,
  followTailRef,
  loadOlderPage,
}: {
  windowKey: string;
  transcript: TranscriptState;
  anchorKey: string | null;
  followTailRef: MutableRefObject<boolean>;
  loadOlderPage: (beforeOffset: number) => Promise<boolean>;
}) {
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const [backfillLoading, setBackfillLoading] = useState(false);
  const backfillInFlightRef = useRef(false);
  const pendingRevealRef = useRef(false);

  const updateWindowAnchor = useCallback((next: string | null) => {
    setTranscriptAnchor(windowKey, next);
  }, [windowKey]);

  const committedRows = useMemo(() => transcriptRows(transcript), [transcript]);
  const resetCutoffMs = useMemo(() => {
    const index = lastSessionResetIndex(committedRows);
    return index >= 0 ? timestampMs(committedRows[index]?.timestamp) : null;
  }, [committedRows]);
  const liveOptimistic = useMemo(() => {
    if (resetCutoffMs == null) return optimistic;
    return optimistic.filter((m) => {
      const ts = timestampMs(m.timestamp);
      return ts == null || ts >= resetCutoffMs;
    });
  }, [optimistic, resetCutoffMs]);

  const allMessages = useMemo(
    () => projectContextEpochs(projectTranscript(transcript, liveOptimistic).rows),
    [transcript, liveOptimistic],
  );

  // Both layers reuse prior objects for unchanged messages so MessageBubble
  // memo keeps skipping the ~220 untouched bubbles on every streamed chunk.
  const timelineRef = useRef<TimelineItem[] | undefined>(undefined);
  const allTimelineItems = useMemo(() => {
    const stabilized = stabilizeTimelineItems(timelineRef.current, reduceToolEvents(allMessages));
    timelineRef.current = stabilized;
    return stabilized;
  }, [allMessages]);

  // A re-keyed or committed anchor row keeps the reading window in the same render,
  // so the tail is never projected under a detached reader in between.
  const anchorIdentityRef = useRef<ChatWindowAnchorIdentity | null>(null);
  const loadedAnchorKey = useMemo(() => {
    if (anchorKey == null) return null;
    if (allTimelineItems.some((item) => timelineItemKey(item) === anchorKey)) return anchorKey;
    const identity = anchorIdentityRef.current;
    return identity ? rekeyedChatWindowAnchor(allTimelineItems, identity) : null;
  }, [allTimelineItems, anchorKey]);
  const anchorIdentity = useMemo(
    () => chatWindowAnchorIdentity(allTimelineItems, loadedAnchorKey),
    [allTimelineItems, loadedAnchorKey],
  );
  anchorIdentityRef.current = anchorIdentity;

  const windowRange = useMemo(
    () => chatWindowRange(allTimelineItems, loadedAnchorKey),
    [allTimelineItems, loadedAnchorKey],
  );
  const windowAnchorIsLoaded = anchorKey == null || loadedAnchorKey != null;

  const projectedTimelineItems = useMemo(
    () => chatWindowProjection(allTimelineItems, loadedAnchorKey).items,
    [allTimelineItems, loadedAnchorKey],
  );
  const enrichmentCacheRef = useRef<EnrichmentCache>(new WeakMap());
  const enrichedMessages = useMemo(
    () => enrichTimelineItems(projectedTimelineItems, enrichmentCacheRef.current),
    [projectedTimelineItems],
  );

  const hiddenMessageCount = transcript.evictedOlder + windowRange.start;
  const backfillCursor = transcriptBackfillCursor(transcript);
  const canBackfillOlder = backfillCursor != null && backfillCursor > 0;

  const loadOlder = useCallback(async () => {
    followTailRef.current = false;
    if (windowAnchorIsLoaded && windowRange.start > 0) {
      updateWindowAnchor(slideChatWindowOlder(allTimelineItems, loadedAnchorKey));
      return;
    }
    if (backfillCursor == null || backfillCursor <= 0 || backfillInFlightRef.current) return;
    backfillInFlightRef.current = true;
    setBackfillLoading(true);
    try {
      if (await loadOlderPage(backfillCursor)) pendingRevealRef.current = true;
    } finally {
      backfillInFlightRef.current = false;
      setBackfillLoading(false);
    }
  }, [
    allTimelineItems,
    backfillCursor,
    followTailRef,
    loadedAnchorKey,
    loadOlderPage,
    updateWindowAnchor,
    windowAnchorIsLoaded,
    windowRange.start,
  ]);

  // Reveal a merged backfill page as soon as it lands in the timeline —
  // state-change driven, never timer driven.
  useLayoutEffect(() => {
    if (!pendingRevealRef.current) return;
    pendingRevealRef.current = false;
    updateWindowAnchor(slideChatWindowOlder(allTimelineItems, loadedAnchorKey));
  }, [allTimelineItems, loadedAnchorKey, updateWindowAnchor]);

  const showLatest = useCallback(() => {
    followTailRef.current = true;
    updateWindowAnchor(null);
  }, [followTailRef, updateWindowAnchor]);

  useLayoutEffect(() => {
    if (anchorKey == null || loadedAnchorKey === anchorKey) return;
    if (loadedAnchorKey == null) showLatest();
    else updateWindowAnchor(loadedAnchorKey);
  }, [anchorKey, loadedAnchorKey, showLatest, updateWindowAnchor]);

  const setHistoryReading = useCallback((reading: boolean) => {
    if (!reading) {
      showLatest();
      return;
    }
    followTailRef.current = false;
    if (anchorKey != null) return;
    const item = allTimelineItems[windowRange.end - 1];
    updateWindowAnchor(item ? timelineItemKey(item) : null);
  }, [allTimelineItems, anchorKey, followTailRef, showLatest, updateWindowAnchor, windowRange.end]);

  // Consumers hand this straight to memoized rows, so it must not churn with the
  // streaming transcript; the ordinal is read from the live one at call time.
  const transcriptRef = useRef(transcript);
  transcriptRef.current = transcript;

  const addOptimistic = useCallback((content: string) => {
    const id = `opt-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
    const clientOrder = nextOptimisticOrder(transcriptRef.current);
    setOptimistic((previous) => [
      ...previous.slice(-(MAX_RETAINED_OPTIMISTIC_MESSAGES - 1)),
      {
        id,
        type: "user",
        content,
        timestamp: String(Date.now()),
        clientOrder,
        // The bubble's own id doubles as its stable send-correlation token; the
        // server echoes it onto the committed turn to reconcile by identity.
        correlationId: id,
      },
    ]);
    return id;
  }, []);

  /** Replace an optimistic bubble's content in place — used to morph a
   * "transcribing…" placeholder into the final transcript (or an error)
   * without the bubble shifting or re-mounting. */
  const updateOptimistic = useCallback((id: string, content: string) => {
    setOptimistic((prev) =>
      prev.map((m) => (m.id === id ? { ...m, content, timestamp: String(Date.now()) } : m)),
    );
  }, []);

  const clearOptimistic = useCallback(() => setOptimistic([]), []);

  useLayoutEffect(() => {
    if (optimistic.length === 0) return;
    const parsedUsers = committedRows.filter(
      (m) => m.type === "user" || m.type === "local_command",
    );
    const { matchedIds } = matchedOptimisticEchoes(toEchoes(optimistic), parsedUsers);
    const matched = new Set(matchedIds);
    setOptimistic((prev) => {
      const next = prev.filter((m) => {
        if (matched.has(m.id)) return false;
        if (resetCutoffMs == null) return true;
        const ts = timestampMs(m.timestamp);
        return ts == null || ts >= resetCutoffMs;
      });
      return next.length === prev.length ? prev : next;
    });
  }, [committedRows, resetCutoffMs, optimistic]);

  return {
    addOptimistic,
    allMessages,
    backfillLoading,
    canBackfillOlder,
    clearOptimistic,
    committedRows,
    enrichedMessages,
    hiddenMessageCount,
    loadOlder,
    setHistoryReading,
    showLatest,
    updateOptimistic,
  };
}

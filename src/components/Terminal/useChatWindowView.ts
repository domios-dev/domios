import { type MutableRefObject, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  lastSessionResetIndex,
  projectContextEpochs,
  projectTranscript,
  reduceToolEvents,
  stabilizeTimelineItems,
  timestampMs,
  transcriptBackfillCursor,
  transcriptRows,
  type TimelineItem,
  type TranscriptState,
} from "@codeterm/chat-core";
import { enrichTimelineItems, type EnrichmentCache } from "./chatEnrichment";
import {
  addChatSend,
  recordChatSendEchoes,
  updateChatSendContent,
  useChatSends,
} from "./chatSendStore";
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

/** Viewport projection over the committed transcript and the tab's sends: window anchor
 *  and enriched rows. It reads both stores and never keeps a copy of either. */
export function useChatWindowView({
  tabId,
  windowKey,
  transcript,
  anchorKey,
  followTailRef,
  loadOlderPage,
}: {
  tabId: string;
  windowKey: string;
  transcript: TranscriptState;
  anchorKey: string | null;
  followTailRef: MutableRefObject<boolean>;
  loadOlderPage: (beforeOffset: number) => Promise<boolean>;
}) {
  const sends = useChatSends(tabId);
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
  const liveSends = useMemo(() => {
    if (resetCutoffMs == null) return sends;
    return sends.filter((send) => send.sentAtMs >= resetCutoffMs);
  }, [sends, resetCutoffMs]);

  const projection = useMemo(() => projectTranscript(transcript, liveSends), [transcript, liveSends]);
  const allMessages = useMemo(() => projectContextEpochs(projection.rows), [projection]);
  useLayoutEffect(() => {
    recordChatSendEchoes(tabId, projection.echoes);
  }, [projection, tabId]);

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

  // The send key is captured here, once, when the composer accepts the send.
  const addOptimistic = useCallback((content: string) => {
    const sentAtMs = Date.now();
    const id = `opt-${sentAtMs}-${Math.floor(Math.random() * 1e6)}`;
    // The bubble's own id doubles as its send-correlation token.
    addChatSend(tabId, { id, content, sentAtMs, correlationId: id });
    return id;
  }, [tabId]);

  const updateOptimistic = useCallback((id: string, content: string) => {
    updateChatSendContent(tabId, id, content);
  }, [tabId]);

  return {
    addOptimistic,
    allMessages,
    backfillLoading,
    canBackfillOlder,
    committedRows,
    enrichedMessages,
    hiddenMessageCount,
    loadOlder,
    setHistoryReading,
    showLatest,
    updateOptimistic,
  };
}

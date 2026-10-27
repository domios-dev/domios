import { useChatHistoryGaps } from "./useChatHistoryGaps";
import { useContextCompaction } from "./useContextCompaction";
import { projectCompactionRows } from "./contextCompactionModel";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { type MutableRefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  lastSessionResetIndex,
  captureSendEchoBoundary,
  projectContextEpochs,
  projectTranscript,
  stabilizeTimelineItems,
  timestampMs,
  transcriptBackfillCursor,
  transcriptRows,
  type NormalizedChatMessage,
  type SendEcho,
  type TimelineItem,
  type TranscriptState,
  type TranscriptAuthority,
} from "@codeterm/chat-core";
import { type EnrichmentCache } from "./chatEnrichment";
import { CHAT_PAGE_SIZE, CHAT_READING_SIZE, conversationEntries, earlierMessageLabel, revealEarlierMessages } from "./chatPagingModel";
import {
  addChatSend,
  readChatSendDismissalMarkers,
  recordChatSendEchoes,
  requestChatSendDismissalMarkers,
  updateChatSendContent,
  useChatSendSnapshot,
} from "./chatSendStore";
import {
  chatSendDismissalIdentity,
  chatSendDismissalMatchedRowKeys,
  type ChatSendDismissalMarker,
} from "./chatSendDurableModel";
import {
  chatWindowAnchorIdentity,
  chatReadingWindowAnchor,
  chatMessageAnchor,
  chatWindowProjection,
  chatWindowRange,
  rekeyedChatWindowAnchor,
  timelineItemKey,
  type ChatWindowAnchorIdentity,
} from "./chatWindow";
import { deliveryProjectionSends, sendForRow, visibleDeliveryRows, type DeliverySend } from "./chatDeliveryModel";
import { useChatSendDismissals } from "./useChatSendDismissals";
import { cancelChatStartPaintTiming, paintChatStartTiming } from "./chatStartTiming";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";
import { setTranscriptAnchor } from "./transcriptStore";
import { searchOlderChatHistory, type OlderChatPage } from "./chatHistorySearch";

/** Viewport projection over the committed transcript and the tab's sends: window anchor
 *  and enriched rows. It reads both stores and never keeps a copy of either. */
export function useChatWindowView({
  tabId,
  agentSessionId = null,
  windowKey,
  transcript,
  attachedAuthority = null,
  anchorKey,
  followTailRef,
  loadOlderPage,
  loadHistoryPage,
}: {
  tabId: string;
  agentSessionId?: string | null;
  windowKey: string;
  transcript: TranscriptState;
  attachedAuthority?: TranscriptAuthority | null;
  anchorKey: string | null;
  followTailRef: MutableRefObject<boolean>;
  loadOlderPage: (beforeOffset: number, maxBytes: number) => Promise<OlderChatPage | null>;
  loadHistoryPage?: (message: NormalizedChatMessage) => Promise<NormalizedChatMessage[] | null>;
}) {
  const { sends, hydration, dismissalRevision } = useChatSendSnapshot(tabId);
  useChatSendDismissals(tabId);
  const [backfillLoading, setBackfillLoading] = useState(false);
  const backfillInFlightRef = useRef(false);
  const [pendingReveal, setPendingReveal] = useState(false);
  const [readingSize, setReadingSize] = useState({ key: windowKey, size: CHAT_PAGE_SIZE });
  const windowSize = readingSize.key === windowKey ? readingSize.size : CHAT_PAGE_SIZE;
  const setWindowSize = (size: number) => setReadingSize({ key: windowKey, size });
  const initialFillRef = useRef<string | null>(null);
  const [backfillNotice, setBackfillNotice] = useState<"limit" | "exhausted" | "stalled_cursor" | null>(null);

  const updateWindowAnchor = useLatestCallback((next: string | null) => {
    setTranscriptAnchor(windowKey, next);
  }, [windowKey]);

  const committedRows = useMemo(() => transcriptRows(transcript), [transcript]);
  const compaction = useContextCompaction(tabId, agentSessionId ?? transcript.authority?.sourceSessionId ?? null, committedRows);
  const resetCutoffMs = useMemo(() => {
    const index = lastSessionResetIndex(committedRows);
    return index >= 0 ? timestampMs(committedRows[index]?.timestamp) : null;
  }, [committedRows]);
  const liveSends = useMemo(() => deliveryProjectionSends(sends, resetCutoffMs), [sends, resetCutoffMs]);
  const projection = useMemo(() => projectTranscript(transcript, liveSends), [transcript, liveSends]);
  const compactDismissals = useMemo(() => hydration === "ready"
    ? readChatSendDismissalMarkers(tabId, projection.rows)
    : EMPTY_DISMISSALS, [dismissalRevision, hydration, projection.rows, tabId]);
  useEffect(() => {
    if (hydration === "ready") requestChatSendDismissalMarkers(tabId, projection.rows);
  }, [hydration, projection.rows, tabId]);
  const allMessages = useMemo(() => {
    const visible = hydration === "ready"
      ? visibleDeliveryRows(projection.rows, sends, projection.echoes)
      : projection.rows;
    return projectContextEpochs(projectCompactionRows(hydration === "ready"
      ? hideCompactedDismissals(visible, sends, projection.echoes, compactDismissals)
      : visible, compaction.history));
  }, [compactDismissals, hydration, projection, sends, compaction.history]);
  useLayoutEffect(() => {
    if (hydration !== "ready") return;
    recordChatSendEchoes(tabId, projection.echoes);
  }, [hydration, projection, tabId]);

  // Both layers reuse prior objects for unchanged messages so MessageBubble
  // memo keeps skipping the ~220 untouched bubbles on every streamed chunk.
  const enrichmentCacheRef = useRef<EnrichmentCache>(new WeakMap());
  const gaps = useChatHistoryGaps(windowKey, loadHistoryPage);
  const baseEntries = useMemo(() => conversationEntries(allMessages, enrichmentCacheRef.current), [allMessages]);
  const gapEntries = useMemo(() => gaps.project(baseEntries), [baseEntries, gaps.project]);
  const leadingGap = gapEntries[0]?.msg.timelineMarker?.kind === "history_gap" ? gapEntries[0].msg : null;
  const allEntries = useMemo(() => leadingGap ? gapEntries.slice(1) : gapEntries, [gapEntries, leadingGap]);
  const timelineRef = useRef<TimelineItem[] | undefined>(undefined);
  const allTimelineItems = useMemo(() => {
    const stabilized = stabilizeTimelineItems(timelineRef.current, allEntries.map(entry => entry.item));
    timelineRef.current = stabilized;
    return stabilized;
  }, [allEntries]);

  // A re-keyed or committed anchor row keeps the reading window in the same render,
  // so the tail is never projected under a detached reader in between.
  const committedTailRef = useRef<ChatWindowAnchorIdentity | null>(null);
  const tailWindowKeyRef = useRef(windowKey);
  if (tailWindowKeyRef.current !== windowKey) {
    tailWindowKeyRef.current = windowKey;
    committedTailRef.current = null;
  }
  const readingAnchorKey = chatReadingWindowAnchor(allTimelineItems, anchorKey, followTailRef.current, committedTailRef.current);
  useLayoutEffect(() => {
    if (!followTailRef.current) {
      if (anchorKey == null && readingAnchorKey != null) updateWindowAnchor(readingAnchorKey);
      return;
    }
    const tail = allTimelineItems[allTimelineItems.length - 1];
    committedTailRef.current = tail ? chatWindowAnchorIdentity(allTimelineItems, timelineItemKey(tail)) : null;
  }, [allTimelineItems, anchorKey, followTailRef, readingAnchorKey, updateWindowAnchor]);

  const anchorIdentityRef = useRef<ChatWindowAnchorIdentity | null>(null);
  const loadedAnchorKey = useMemo(() => {
    if (readingAnchorKey == null) return null;
    if (allTimelineItems.some((item) => timelineItemKey(item) === readingAnchorKey)) return readingAnchorKey;
    const identity = anchorIdentityRef.current;
    return identity ? rekeyedChatWindowAnchor(allTimelineItems, identity) : null;
  }, [allTimelineItems, readingAnchorKey]);
  const anchorIdentity = useMemo(
    () => chatWindowAnchorIdentity(allTimelineItems, loadedAnchorKey),
    [allTimelineItems, loadedAnchorKey],
  );
  anchorIdentityRef.current = anchorIdentity;

  const windowRange = useMemo(
    () => chatWindowRange(allTimelineItems, loadedAnchorKey, windowSize),
    [allTimelineItems, loadedAnchorKey, windowSize],
  );

  const windowProjection = useMemo(
    () => chatWindowProjection(allTimelineItems, loadedAnchorKey, windowSize),
    [allTimelineItems, loadedAnchorKey, windowSize],
  );
  const diagnosticTail = allTimelineItems.length > 0 ? timelineItemKey(allTimelineItems[allTimelineItems.length - 1]) : null;
  useLayoutEffect(() => {
    chatScrollDiagnostic(windowKey, "window", { anchorKey, loadedAnchorKey, newerCount: windowProjection.newerCount,
      followTail: followTailRef.current, tail: diagnosticTail });
  }, [anchorKey, loadedAnchorKey, windowProjection.newerCount, diagnosticTail, followTailRef, windowKey]);
  const enrichedMessages = useMemo(() => {
    const byId = new Map(allEntries.map(entry => [entry.msg.id, entry]));
    return windowProjection.items.map(item => byId.get(timelineItemKey(item))!);
  }, [allEntries, windowProjection]);

  useLayoutEffect(() => {
    if (enrichedMessages.length === 0) return;
    paintChatStartTiming(tabId);
  });

  useLayoutEffect(() => () => cancelChatStartPaintTiming(tabId), [tabId]);

  const hiddenMessageCount = windowRange.start;
  const backfillCursor = transcriptBackfillCursor(transcript);
  const canReadServer = backfillCursor != null && backfillCursor > 0;
  const canBackfillOlder = canReadServer || leadingGap != null;
  const olderMessageLabel = earlierMessageLabel(hiddenMessageCount, canBackfillOlder);

  const revealOlder = useLatestCallback(() => {
    const next = revealEarlierMessages(allTimelineItems, loadedAnchorKey, windowSize);
    setWindowSize(next.size);
    updateWindowAnchor(next.anchor);
  }, [allTimelineItems, loadedAnchorKey, windowSize, updateWindowAnchor]);

  const loadOlder = useLatestCallback(async (initial = false) => {
    if (backfillInFlightRef.current) return;
    if (!initial) followTailRef.current = false;
    setBackfillNotice(null);
    if (hiddenMessageCount >= CHAT_PAGE_SIZE || !canBackfillOlder) {
      if (!initial) revealOlder();
      return;
    }
    const first = allTimelineItems[windowRange.start];
    const firstKey = first ? timelineItemKey(first) : null;
    const tail = allTimelineItems[windowRange.end - 1];
    if (!initial) {
      setWindowSize(windowRange.end - windowRange.start);
      if (tail) updateWindowAnchor(timelineItemKey(tail));
    }
    backfillInFlightRef.current = true;
    setBackfillLoading(true);
    try {
      if (!canReadServer && leadingGap) {
        await gaps.load(leadingGap);
        if (!initial) setPendingReveal(true);
        return;
      }
      const result = await searchOlderChatHistory(backfillCursor!, loadOlderPage, undefined, page => {
        if (!page.messages) return page.rows >= CHAT_PAGE_SIZE;
        const entries = conversationEntries(page.messages);
        return firstKey == null ? entries.length >= CHAT_PAGE_SIZE
          : entries.findIndex(entry => entry.msg.id === firstKey) >= CHAT_PAGE_SIZE;
      });
      if (!initial && (result.kind === "reveal" || result.kind === "stop")) setPendingReveal(true);
      if (result.kind === "stop" || result.kind === "error") setBackfillNotice(result.reason);
    } catch {
      setBackfillNotice("stalled_cursor");
    } finally {
      backfillInFlightRef.current = false;
      setBackfillLoading(false);
    }
  }, [allTimelineItems, backfillCursor, canBackfillOlder, followTailRef, hiddenMessageCount,
    canReadServer, gaps.load, leadingGap, loadOlderPage, revealOlder, updateWindowAnchor, windowRange]);

  useLayoutEffect(() => {
    if (!pendingReveal) return;
    setPendingReveal(false);
    revealOlder();
  }, [allTimelineItems, pendingReveal, revealOlder]);

  useEffect(() => {
    if (initialFillRef.current === windowKey || transcript.authority == null) return;
    initialFillRef.current = windowKey;
    if (allTimelineItems.length >= CHAT_PAGE_SIZE || !canBackfillOlder || backfillInFlightRef.current) return;
    void loadOlder(true);
  }, [allTimelineItems.length, backfillCursor, canBackfillOlder, loadOlder, loadOlderPage, transcript.authority, windowKey]);

  const showLatest = useLatestCallback(() => {
    followTailRef.current = true;
    setWindowSize(CHAT_PAGE_SIZE);
    updateWindowAnchor(null);
  }, [followTailRef, updateWindowAnchor]);

  const prepareMessage = useLatestCallback((messageId: string) => {
    const anchor = chatMessageAnchor(allTimelineItems, messageId);
    if (!anchor || loadedAnchorKey === anchor) return false;
    followTailRef.current = false;
    updateWindowAnchor(anchor);
    return true;
  }, [allTimelineItems, loadedAnchorKey, followTailRef, updateWindowAnchor]);

  useLayoutEffect(() => {
    if (anchorKey == null || loadedAnchorKey === anchorKey) return;
    if (loadedAnchorKey == null) showLatest();
    else updateWindowAnchor(loadedAnchorKey);
  }, [anchorKey, loadedAnchorKey, showLatest, updateWindowAnchor]);

  const setHistoryReading = useLatestCallback((reading: boolean) => {
    if (!reading) {
      showLatest();
      return;
    }
    followTailRef.current = false;
    if (anchorKey != null) return;
    const item = allTimelineItems[windowRange.end - 1];
    updateWindowAnchor(readingAnchorKey ?? (item ? timelineItemKey(item) : null));
  }, [allTimelineItems, anchorKey, followTailRef, readingAnchorKey, showLatest, updateWindowAnchor, windowRange.end]);

  // The send key is captured here, once, when the composer accepts the send.
  const addOptimistic = useLatestCallback((content: string) => {
    const sentAtMs = Date.now();
    const id = `opt-${sentAtMs}-${Math.floor(Math.random() * 1e6)}`;
    // The bubble's own id doubles as its send-correlation token.
    addChatSend(tabId, { id, content, sentAtMs, correlationId: id,
      echoBoundary: captureSendEchoBoundary(transcript.authority ?? attachedAuthority, committedRows) });
    return id;
  }, [tabId]);

  const updateOptimistic = useLatestCallback((id: string, content: string) => {
    updateChatSendContent(tabId, id, content);
  }, [tabId]);

  return {
    addOptimistic,
    compactionFailure: compaction.failure ? `Compaction history could not be saved: ${compaction.failure}` : undefined,
    allMessages,
    backfillLoading,
    backfillNotice: backfillNotice ?? (initialFillRef.current === windowKey && !backfillLoading
      && canBackfillOlder && enrichedMessages.length < CHAT_PAGE_SIZE ? "limit" : null),
    canBackfillOlder,
    committedRows,
    enrichedMessages,
    hiddenMessageCount,
    olderMessageLabel,
    historyGapLabel: gaps.label,
    loadHistoryGap: async (message: NormalizedChatMessage) => {
      followTailRef.current = false;
      const tail = allTimelineItems[windowRange.end - 1];
      if (tail) updateWindowAnchor(timelineItemKey(tail));
      const count = await gaps.load(message);
      if (count > 0) setWindowSize(Math.min(CHAT_READING_SIZE, windowSize + count));
    },
    newerMessageCount: windowProjection.newerCount,
    loadOlder,
    setHistoryReading,
    showLatest,
    prepareMessage,
    updateOptimistic,
  };
}

const EMPTY_DISMISSALS: readonly ChatSendDismissalMarker[] = [];

function hideCompactedDismissals(
  rows: NormalizedChatMessage[],
  sends: readonly DeliverySend[],
  echoes: ReadonlyMap<string, SendEcho>,
  markers: readonly ChatSendDismissalMarker[],
): NormalizedChatMessage[] {
  if (markers.length === 0) return rows;
  const withEchoes = sends.map((send) => ({ ...send, echo: send.echo ?? echoes.get(send.id) }));
  const ids = new Set(markers.map((marker) => marker.sendId));
  const correlations = new Set(markers.flatMap((marker) => marker.correlationId ? [marker.correlationId] : []));
  const matchedRows = new Set(markers.flatMap((marker) => marker.matchedRowKey ? [marker.matchedRowKey] : []));
  const echoKeys = new Set(markers.flatMap((marker) => marker.echoKey ? [marker.echoKey] : []));
  return rows.filter((row) => {
    if (sendForRow(withEchoes, row)) return true;
    const identity = chatSendDismissalIdentity(row);
    if (!identity) return true;
    if (ids.has(identity.id) || (identity.correlationId && correlations.has(identity.correlationId))) return false;
    if (chatSendDismissalMatchedRowKeys(identity).some((key) => matchedRows.has(key))) return false;
    return !identity.echoKey || !echoKeys.has(identity.echoKey);
  });
}

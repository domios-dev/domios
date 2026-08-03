import {
  type MutableRefObject,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  createTranscriptState,
  reduceTranscript,
  type TranscriptBatch,
  type TranscriptOutcome,
  type TranscriptState,
} from "@codeterm/chat-core";
import {
  getCachedChatHistoryWindow,
  setCachedChatHistoryWindow,
} from "./chatWindow";

/** Committed transcript for one tab+session, cached across a Terminal <-> Chat
 *  switch. Every mutation goes through the transcript reducer. */
export function useChatHistoryRetention({
  tabId,
  agentSessionId,
  suppliedFollowTailRef,
}: {
  tabId: string;
  agentSessionId: string | null;
  suppliedFollowTailRef?: MutableRefObject<boolean>;
}) {
  const historyCacheKey = `${tabId}:${agentSessionId ?? ""}`;
  const [initialCachedWindow] = useState(() => getCachedChatHistoryWindow(historyCacheKey));
  const [transcript, setTranscript] = useState<TranscriptState>(
    () => initialCachedWindow?.transcript ?? createTranscriptState(),
  );
  const [windowAnchorKey, setWindowAnchorKey] = useState<string | null>(
    initialCachedWindow?.anchorKey ?? null,
  );
  const internalFollowTailRef = useRef(true);
  const followTailRef = suppliedFollowTailRef ?? internalFollowTailRef;
  const transcriptCacheKeyRef = useRef(historyCacheKey);
  const committedRef = useRef(transcript);

  const applyTranscript = useCallback((next: TranscriptState) => {
    committedRef.current = next;
    setTranscript(next);
  }, []);

  /** Returns the reducer outcome so the caller can act on a gap or overlap. */
  const commitTranscript = useCallback((batch: TranscriptBatch): TranscriptOutcome => {
    const { state, outcome } = reduceTranscript(committedRef.current, batch);
    if (state !== committedRef.current) applyTranscript(state);
    return outcome;
  }, [applyTranscript]);

  const resetTranscript = useCallback(() => {
    applyTranscript(createTranscriptState());
  }, [applyTranscript]);

  const updateWindowAnchor = useCallback((action: SetStateAction<string | null>) => {
    setWindowAnchorKey((previous) => (
      typeof action === "function" ? action(previous) : action
    ));
  }, []);

  useEffect(() => {
    if (transcriptCacheKeyRef.current !== historyCacheKey) return;
    setCachedChatHistoryWindow(
      historyCacheKey,
      transcript.segments.length > 0 ? { transcript, anchorKey: windowAnchorKey } : null,
    );
  }, [historyCacheKey, transcript, windowAnchorKey]);

  useEffect(() => {
    if (transcriptCacheKeyRef.current === historyCacheKey) return;
    transcriptCacheKeyRef.current = historyCacheKey;
    const cached = getCachedChatHistoryWindow(historyCacheKey);
    followTailRef.current = cached?.anchorKey == null;
    applyTranscript(cached?.transcript ?? createTranscriptState());
    updateWindowAnchor(cached?.anchorKey ?? null);
  }, [applyTranscript, followTailRef, historyCacheKey, updateWindowAnchor]);

  return {
    commitTranscript,
    followTailRef,
    initialCachedWindow,
    resetTranscript,
    transcript,
    updateWindowAnchor,
    windowAnchorKey,
  };
}

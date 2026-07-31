import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { canonicalBlob, displayablePreview, uncoveredPreviewTail } from "./streamPreviewMatch";

/** Cumulative in-flight turn reconstruction emitted by the daemon's motion
 * decoder. Idempotent: `frozen` and `live` replace previous state entirely. */
export interface StreamPreviewEventPayload {
  tab_id: string;
  turn_seq: number;
  frozen: string[];
  live: string[];
}

const MAX_RENDER_ROWS = 8;

/**
 * Ephemeral streaming preview for PTY agent tabs: the decoder's turn view
 * minus everything already covered by canonical messages. The covered prefix
 * disappears as JSONL turns land; the uncovered tail keeps streaming. Never a
 * canonical source — purely display.
 */
export function useStreamPreview(
  tabId: string,
  active: boolean,
  agentWorking: boolean,
  canonicalContents: readonly string[],
): string[] {
  const [event, setEvent] = useState<StreamPreviewEventPayload | null>(null);

  useEffect(() => {
    if (!active) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("chat-stream-preview", (ev: { payload: StreamPreviewEventPayload }) => {
      if (ev.payload.tab_id !== tabId) return;
      setEvent(ev.payload);
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tabId, active]);

  const blob = useMemo(() => canonicalBlob(canonicalContents), [canonicalContents]);
  const displayed = useMemo(() => {
    if (!event) return [];
    const content = displayablePreview(event.frozen.concat(event.live));
    return uncoveredPreviewTail(content, blob).slice(-MAX_RENDER_ROWS);
  }, [event, blob]);

  // Turn over and fully covered by canon → drop the stale event entirely.
  useEffect(() => {
    if (!agentWorking && event !== null && displayed.length === 0) setEvent(null);
  }, [agentWorking, displayed.length, event]);

  // The swap: once the turn is over, the next canonical update IS the final
  // content — the preview dies unconditionally instead of lingering as a dupe.
  const lastBlobRef = useRef(blob);
  useEffect(() => {
    const blobChanged = lastBlobRef.current !== blob;
    lastBlobRef.current = blob;
    if (blobChanged && !agentWorking) setEvent(null);
  }, [blob, agentWorking]);

  return displayed;
}

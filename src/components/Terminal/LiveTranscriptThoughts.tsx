import { useEffect, useLayoutEffect, useRef, useState, type SyntheticEvent } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import { reduceToolEvents, type ChatMsg, type NormalizedChatMessage } from "@codeterm/chat-core";
import { THOUGHTS_PAGE_SIZE, type LiveTranscriptApi } from "../../lib/transport/liveTranscript";
import { BusyState } from "../shared/BusyState";
import { errorText } from "./useLiveTranscriptPanel";

function scopedRows(rows: ChatMsg[], epoch: string): NormalizedChatMessage[] {
  return rows.flatMap(row => {
    const kind = row.type;
    if (kind !== "thinking" && kind !== "tool" && kind !== "tool_call" && kind !== "tool_result") return [];
    const {
      id, content, seq, timestamp, toolId, toolName, toolArgs, toolDescription, toolTitle, toolSummary, toolDetail,
      toolResult, toolError, toolExitCode, toolDurationMs, toolCallSourceId, toolPairing,
    } = row;
    return [{
      id, content, seq, timestamp, type: kind === "tool" ? "tool_call" : kind, sourceSessionId: epoch,
      toolId, toolName, toolArgs, toolDescription, toolTitle, toolSummary, toolDetail, toolResult,
      toolError, toolExitCode, toolDurationMs, toolCallSourceId,
      toolPairing: toolPairing === "exact" || toolPairing === "unavailable" ? toolPairing : undefined,
    }];
  });
}

function preventNavigation(event: SyntheticEvent) {
  if (event.target instanceof Element && event.target.closest("a")) {
    event.preventDefault();
    event.stopPropagation();
  }
}

function useEpochPage(api: LiveTranscriptApi, epoch: string) {
  const [offset, setOffset] = useState(0);
  const [revision, setRevision] = useState(0);
  const [page, setPage] = useState<{ offset: number; rows: NormalizedChatMessage[]; next: number | null }>();
  const [error, setError] = useState<string>();
  const scrollRef = useRef<HTMLDivElement>(null);
  const lastOffset = useRef<number | undefined>(undefined);
  const followTail = useRef(false);
  useEffect(() => {
    const request = new AbortController();
    api.epoch(epoch, offset, request.signal).then(response => {
      if (request.signal.aborted) return;
      setPage({ offset, rows: scopedRows(response.rows.slice(0, THOUGHTS_PAGE_SIZE), epoch), next: response.nextOffset });
      setError(undefined);
    }).catch(failure => {
      if (!request.signal.aborted) setError(errorText(failure));
    });
    return () => request.abort();
  }, [api, epoch, offset, revision]);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !page) return;
    if (lastOffset.current !== page.offset) {
      followTail.current = false;
      el.scrollTop = 0;
    }
    else if (followTail.current) el.scrollTop = el.scrollHeight;
    lastOffset.current = page.offset;
  }, [page]);
  function onScroll() {
    const el = scrollRef.current;
    if (el) followTail.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }
  return { page: page?.offset === offset ? page : undefined, offset, setOffset, error, scrollRef, onScroll,
    refresh: () => setRevision(value => value + 1) };
}

export function LiveTranscriptThoughts({ api, epoch, rationale, onClose }: {
  api: LiveTranscriptApi;
  epoch: string;
  rationale?: string;
  onClose: () => void;
}) {
  const view = useEpochPage(api, epoch);
  const items = reduceToolEvents(view.page?.rows ?? []);
  return <section className="ct-live-thoughts" aria-label="Thoughts inspector">
    <div className="ct-live-toolbar">
      <strong>Thoughts</strong>
      <button type="button" onClick={view.refresh}>Refresh Thoughts</button>
      <button type="button" onClick={onClose} aria-label="Close Thoughts">Close</button>
    </div>
    {rationale && <p className="ct-live-rationale">{rationale}</p>}
    <div ref={view.scrollRef} onScroll={view.onScroll} className="ct-live-thoughts-scroll"
      onClickCapture={preventNavigation} onAuxClickCapture={preventNavigation} onContextMenuCapture={preventNavigation}>
      {!view.page && !view.error && <BusyState inline label="Reading Thoughts…" phase="work" />}
      {view.error && <p role="status">Reasoning unavailable. {view.error}</p>}
      {view.page && !view.page.rows.some(row => row.type === "thinking" && row.content.trim()) &&
        <p role="status">Provider-published reasoning is unavailable on this page.</p>}
      {items.map(item => {
        const message = item.kind === "message" ? item.message : item.event.message;
        return <MessageBubble key={message.id} message={message} item={item} />;
      })}
    </div>
    {view.page && (view.offset > 0 || view.page.next != null) && <div className="ct-live-toolbar">
      <button type="button" disabled={view.offset === 0}
        onClick={() => view.setOffset(Math.max(0, view.offset - THOUGHTS_PAGE_SIZE))}>Previous Thoughts</button>
      <button type="button" disabled={view.page.next == null}
        onClick={() => { if (view.page?.next != null) view.setOffset(view.page.next); }}>Next Thoughts</button>
    </div>}
  </section>;
}

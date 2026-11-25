import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { MessageBubble } from "@codeterm/chat-ui";
import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import type { EnrichmentCache } from "../Terminal/chatEnrichment";
import { conversationEntries } from "../Terminal/chatPagingModel";
import { LiveRunTranscript } from "./useRunTranscript";
import { invoke } from "../../lib/bridge";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { cronRunTranscriptRequest } from "./cronRunTranscriptRequest";

interface Props {
  title: string;
  subtitle?: string;
  status: string;
  report?: string | null;
  provider: string;
  cwd: string;
  sessionId: string;
  tabId?: string | null;
  cronId?: string;
  runId?: string;
  ownerInstanceId?: string | null;
  live?: boolean;
  onClose: () => void;
}

export function RunChatPopup({
  title,
  subtitle,
  status,
  report,
  provider,
  cwd,
  sessionId,
  live,
  tabId,
  cronId,
  runId,
  ownerInstanceId,
  onClose,
}: Props) {
  const [detachedSession, setDetachedSession] = useState<string | null>(null);
  const watching = Boolean(live && tabId && detachedSession !== sessionId);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const enrichmentCache = useRef<EnrichmentCache>(new WeakMap());
  const entries = useMemo(() => conversationEntries(messages, enrichmentCache.current, cwd), [messages, cwd]);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);
  const stickyBottomRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    if (watching) return;
    setMessages([]);
    setError(null);
    setLoaded(false);
    const load = () => {
      const request = cronId && runId
        ? cronRunTranscriptRequest(cronId, { run_id: runId, owner_instance_id: ownerInstanceId ?? undefined }, ownerInstanceId)
        : null;
      const transcript = request
        ? daemonTransportForPeer(request.targetInstanceId).json<{ messages?: ChatMessage[] }>("GET", request.path)
        : invoke<{ messages?: ChatMessage[] }>("session_transcript", { provider, cwd, sessionId });
      transcript
        .then((res) => {
          if (cancelled) return;
          setMessages(Array.isArray(res.messages) ? res.messages : []);
          setError(null);
          setLoaded(true);
        })
        .catch((e) => {
          if (!cancelled) {
            setError(String(e));
            setLoaded(true);
          }
        });
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [provider, cwd, sessionId, cronId, runId, ownerInstanceId, watching]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
      stickyBottomRef.current = dist < 60;
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, []);
  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickyBottomRef.current) el.scrollTop = el.scrollHeight;
  }, [messages]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return createPortal(
    <div
      ref={backdropRef}
      onClick={(e) => {
        if (e.target === backdropRef.current) onClose();
      }}
      className="fixed inset-0 z-[60] flex items-center justify-center backdrop-blur-sm"
      style={{ background: "rgba(0,0,0,0.65)" }}
    >
      <div
        className="flex flex-col bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg shadow-2xl"
        style={{ width: "min(820px, 90vw)", height: "min(640px, 85vh)" }}
      >
        <div className="ct-jobs-header rounded-t-[inherit] flex items-center justify-between px-4 py-2.5">
          <div className="flex items-center gap-2 min-w-0">
            <span className="text-xs font-medium truncate text-[var(--ct-fg)]">{title}</span>
            {subtitle && (
              <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0">{subtitle}</span>
            )}
            {!live && (
              <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0">{status}</span>
            )}
          </div>
          <button
            onClick={onClose}
            className="bg-transparent border-0 text-[var(--ct-muted-fg)] cursor-pointer text-sm px-1 hover:text-[var(--ct-fg)]"
          >
            ✕
          </button>
        </div>

        {watching && tabId && <LiveRunTranscript tabId={tabId} sessionId={sessionId} onMessages={setMessages} onError={setError} onLoaded={setLoaded} onDetached={setDetachedSession} />}
        <div ref={scrollRef} className="relative flex-1 overflow-auto px-4 py-3">
          {error && <InlineErrorNotice error={error} operation="reading the agent conversation" />}
          {loaded && !error && entries.length === 0 && (
            <div className="text-[var(--ct-muted-fg)] text-[11px]">No transcript for this run.</div>
          )}
          {entries.map(({ msg, item, origin }) => (
            <MessageBubble key={msg.id} message={msg} origin={origin} item={item.kind === "tool" ? item : undefined} />
          ))}
        </div>

        {report && (
          <div className="px-4 py-2 text-[10px] text-[var(--ct-muted-fg)]">
            <strong>Report:</strong> {report}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}

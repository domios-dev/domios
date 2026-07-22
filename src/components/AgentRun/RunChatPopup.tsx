import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { MessageBubble } from "@codeterm/chat-ui";
import { reduceToolEvents, type NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { invoke } from "../../lib/bridge";

interface Props {
  title: string;
  subtitle?: string;
  status: string;
  report?: string | null;
  provider: string;
  cwd: string;
  sessionId: string;
  // When set the run is still live — poll the transcript until the popup closes.
  live?: boolean;
  onClose: () => void;
}

const POLL_MS = 1500;

/// Read-only chat-ui for a hidden worker / cron run, rendered straight from the
/// session jsonl via the session_transcript command. Same MessageBubble as the
/// live chat — just no composer.
export function RunChatPopup({
  title,
  subtitle,
  status,
  report,
  provider,
  cwd,
  sessionId,
  live,
  onClose,
}: Props) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);
  const stickyBottomRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      invoke<{ messages?: ChatMessage[] }>("session_transcript", {
        provider,
        cwd,
        sessionId,
      })
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
    const timer = live ? window.setInterval(load, POLL_MS) : null;
    return () => {
      cancelled = true;
      if (timer) window.clearInterval(timer);
    };
  }, [provider, cwd, sessionId, live]);

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
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--ct-border)]">
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

        <div ref={scrollRef} className="relative flex-1 overflow-auto px-4 py-3">
          {error && <div className="text-[var(--ct-red)] mb-2 text-[11px]">{error}</div>}
          {loaded && !error && messages.length === 0 && (
            <div className="text-[var(--ct-muted-fg)] text-[11px]">No transcript for this run.</div>
          )}
          {reduceToolEvents(messages).map((item) => {
            const m = item.kind === "message" ? item.message : item.event.message;
            return <MessageBubble key={m.id} message={m} item={item.kind === "tool" ? item : undefined} />;
          })}
        </div>

        {report && (
          <div className="px-4 py-2 border-t border-[var(--ct-border)] text-[10px] text-[var(--ct-muted-fg)]">
            <strong>Report:</strong> {report}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}

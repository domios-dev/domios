import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { MessageBubble } from "@codeterm/provider-claude/src/MessageBubble";
import {
  parseJsonlSession,
  type ChatMessage,
} from "@codeterm/provider-claude/src/parser";

// Optimistic messages auto-prune after this if the JSONL never echoes
// them back — catches sends that silently dropped (claude in a permission
// dialog, paste mis-targeted, agent crashed). User can still see them
// during the window so they know the click registered.
const OPTIMISTIC_TTL_MS = 60_000;

interface ChatStartResult {
  text: string;
  offset: number;
}

interface RawDeltaEvent {
  pane_id: string;
  text: string;
}

interface Props {
  paneId: string;
  onError?: (msg: string) => void;
}

export function PaneSessionChat({ paneId, onError }: Props) {
  const [rawText, setRawText] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    (async () => {
      try {
        const res = await invoke<ChatStartResult>("chat_start", { paneId });
        if (cancelled) return;
        setRawText(res.text || "");
        setLoading(false);
        unlisten = await listen<RawDeltaEvent>(
          `pane-chat-raw-${paneId}`,
          (ev) => {
            if (cancelled) return;
            const newText = ev.payload.text || "";
            if (!newText) return;
            setRawText((prev) => prev + newText);
          },
        );
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
        setLoading(false);
        onError?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      void invoke("chat_stop", { paneId }).catch(() => {});
    };
  }, [paneId, onError]);

  const parsed = useMemo(() => parseJsonlSession(rawText), [rawText]);

  // Drop optimistic messages whose content APPEARS INSIDE the parsed
  // stream. Substring match (not equality) because claude often wraps
  // the user's raw input with extra context (env reminders, system-
  // reminder blocks, attachment metadata) — strict equality would
  // leave the optimistic message stuck forever.
  const matchesParsedUser = useCallback(
    (optimisticContent: string, recentParsed: ChatMessage[]) => {
      const trimmed = optimisticContent.trim();
      if (!trimmed) return false;
      return recentParsed.some(
        (p) => p.type === "user" && p.content.includes(trimmed),
      );
    },
    [],
  );

  const allMessages = useMemo(() => {
    if (optimistic.length === 0) return parsed;
    const tail = Math.max(optimistic.length * 4, 20);
    const recentParsed = parsed.slice(-tail);
    const stillPending = optimistic.filter(
      (m) => !matchesParsedUser(m.content, recentParsed),
    );
    return [...parsed, ...stillPending];
  }, [parsed, optimistic, matchesParsedUser]);

  // Garbage-collect optimistic state once a parsed user message contains
  // it, AND drop optimistic entries older than OPTIMISTIC_TTL_MS as a
  // safety net — if the send silently dropped (claude in permission
  // dialog, paste mis-targeted, agent crashed) we don't want a stale
  // bubble pinned at the bottom forever.
  useEffect(() => {
    if (optimistic.length === 0) return;
    const now = Date.now();
    const tail = Math.max(optimistic.length * 4, 20);
    const recentParsed = parsed.slice(-tail);
    setOptimistic((prev) => {
      const next = prev.filter((m) => {
        if (matchesParsedUser(m.content, recentParsed)) return false;
        if (m.timestamp) {
          const sentAt = Number(m.timestamp);
          if (!Number.isNaN(sentAt) && now - sentAt > OPTIMISTIC_TTL_MS) {
            return false;
          }
        }
        return true;
      });
      return next.length === prev.length ? prev : next;
    });
  }, [parsed, optimistic.length, matchesParsedUser]);

  useEffect(() => {
    if (!scrollRef.current) return;
    if (atBottomRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [allMessages]);

  // Tick the TTL-pruner every 10s so stale optimistic messages clear
  // even if no JSONL chunks have arrived to nudge the other effect.
  useEffect(() => {
    if (optimistic.length === 0) return;
    const id = setInterval(() => {
      const now = Date.now();
      setOptimistic((prev) => {
        const next = prev.filter((m) => {
          if (!m.timestamp) return true;
          const sentAt = Number(m.timestamp);
          if (Number.isNaN(sentAt)) return true;
          return now - sentAt <= OPTIMISTIC_TTL_MS;
        });
        return next.length === prev.length ? prev : next;
      });
    }, 10_000);
    return () => clearInterval(id);
  }, [optimistic.length]);

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    setSending(true);
    setOptimistic((prev) => [
      ...prev,
      {
        id: `opt-${Date.now()}`,
        type: "user",
        content: text,
        // `timestamp` doubles as the optimistic-sentAt clock — we read
        // it back as a number for TTL pruning. ChatMessage.timestamp is
        // typed `string` so we stringify; the millis fit comfortably.
        timestamp: String(Date.now()),
      },
    ]);
    setInput("");
    try {
      await invoke("chat_send_text", { paneId, text, enter: true });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
  }

  if (error) {
    return (
      <div
        className="w-full h-full flex items-center justify-center p-4 text-sm"
        style={{ color: "var(--ct-muted-fg, #8b949e)" }}
      >
        <div className="text-center">
          <div className="mb-2">UI mode unavailable for this pane.</div>
          <div className="text-xs opacity-60">{error}</div>
        </div>
      </div>
    );
  }

  if (loading) {
    return <div className="p-4 text-sm opacity-60">Loading chat…</div>;
  }

  return (
    <div
      className="w-full h-full flex flex-col"
      style={{
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
    >
      <div
        ref={scrollRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          const dist = el.scrollHeight - (el.scrollTop + el.clientHeight);
          atBottomRef.current = dist < 50;
        }}
        className="flex-1 min-h-0 overflow-y-auto p-3"
      >
        {allMessages.length === 0 && (
          <div className="opacity-50 text-center py-8 text-sm">
            No messages yet.
          </div>
        )}
        {allMessages.map((m) => {
          const isPending = m.id.startsWith("opt-");
          return (
            <div
              key={m.id}
              style={isPending ? { opacity: 0.55 } : undefined}
              title={isPending ? "sending… (echo expected within 60s)" : undefined}
            >
              <MessageBubble message={m} />
            </div>
          );
        })}
      </div>
      <div
        className="shrink-0 p-2 border-t"
        style={{
          borderColor:
            "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
        }}
      >
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
          placeholder="Type a message…  (Enter to send, Shift+Enter for newline)"
          rows={2}
          disabled={sending}
          className="w-full resize-none rounded p-2 text-sm font-mono outline-none"
          style={{
            background:
              "color-mix(in srgb, var(--ct-bg, #0d1117) 80%, white 5%)",
            color: "var(--ct-fg, #c9d1d9)",
            border:
              "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
          }}
        />
      </div>
    </div>
  );
}

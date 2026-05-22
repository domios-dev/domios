import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { MessageBubble } from "@codeterm/provider-claude/src/MessageBubble";
import {
  parseJsonlSession,
  type ChatMessage,
} from "@codeterm/provider-claude/src/parser";

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

  // Drop optimistic messages whose content already appears in the parsed
  // stream. The user's typed text lands in the JSONL as a `user` message
  // ~500ms-1s later — once it's there, the optimistic stand-in is
  // redundant and would render as a duplicate.
  const recentUserContents = useMemo(() => {
    if (parsed.length === 0) return new Set<string>();
    const tail = Math.max(optimistic.length * 3, 10);
    return new Set(
      parsed
        .slice(-tail)
        .filter((m) => m.type === "user")
        .map((m) => m.content),
    );
  }, [parsed, optimistic.length]);

  const allMessages = useMemo(() => {
    if (optimistic.length === 0) return parsed;
    const stillPending = optimistic.filter(
      (m) => !recentUserContents.has(m.content),
    );
    return [...parsed, ...stillPending];
  }, [parsed, optimistic, recentUserContents]);

  // Garbage-collect optimistic state once the JSONL catches up — keeps
  // the list bounded across many sends.
  useEffect(() => {
    if (optimistic.length === 0) return;
    setOptimistic((prev) =>
      prev.filter((m) => !recentUserContents.has(m.content)),
    );
  }, [recentUserContents, optimistic.length]);

  useEffect(() => {
    if (!scrollRef.current) return;
    if (atBottomRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [allMessages]);

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    setSending(true);
    setOptimistic((prev) => [
      ...prev,
      { id: `opt-${Date.now()}`, type: "user", content: text },
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
        {allMessages.map((m) => (
          <MessageBubble key={m.id} message={m} />
        ))}
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

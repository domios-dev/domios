import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  RichMessage,
  ChatStartResult,
  ChatDeltaEvent,
} from "../../lib/paneChatTypes";

interface Props {
  paneId: string;
  onError?: (msg: string) => void;
}

export function PaneSessionChat({ paneId, onError }: Props) {
  const [msgs, setMsgs] = useState<RichMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    (async () => {
      try {
        const res = await invoke<ChatStartResult>("chat_start", { paneId });
        if (cancelled) return;
        setMsgs(res.snapshot || []);
        setLoading(false);
        unlisten = await listen<ChatDeltaEvent>(
          `pane-chat-${paneId}`,
          (ev) => {
            if (cancelled) return;
            const newMsgs = ev.payload.messages || [];
            if (newMsgs.length === 0) return;
            setMsgs((prev) => [...prev, ...newMsgs]);
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

  // Auto-scroll to bottom when new messages arrive AND user was at bottom.
  useEffect(() => {
    if (!scrollRef.current) return;
    if (atBottomRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [msgs]);

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
      ref={scrollRef}
      onScroll={(e) => {
        const el = e.currentTarget;
        const distFromBottom = el.scrollHeight - (el.scrollTop + el.clientHeight);
        atBottomRef.current = distFromBottom < 50;
      }}
      className="w-full h-full overflow-y-auto p-3 text-sm font-mono"
      style={{ background: "var(--ct-bg, #0d1117)", color: "var(--ct-fg, #c9d1d9)" }}
    >
      {msgs.length === 0 && (
        <div className="opacity-50 text-center py-8">No messages yet.</div>
      )}
      {msgs.map((m, i) => (
        <MessageRow key={i} msg={m} />
      ))}
    </div>
  );
}

function MessageRow({ msg }: { msg: RichMessage }) {
  const roleStyle =
    msg.role === "user"
      ? { color: "var(--ct-accent, #58a6ff)" }
      : { color: "var(--ct-fg, #c9d1d9)" };
  return (
    <div className="mb-3">
      <div
        className="text-xs uppercase tracking-wider opacity-60 mb-1"
        style={roleStyle}
      >
        {msg.role}
      </div>
      <div className="whitespace-pre-wrap break-words pl-2">
        {msg.blocks.map((b, j) => (
          <BlockRender key={j} block={b} />
        ))}
      </div>
    </div>
  );
}

function BlockRender({
  block,
}: {
  block: { kind: string; data: Record<string, unknown> };
}) {
  if (block.kind === "text") {
    return <span>{String(block.data.text ?? "")}</span>;
  }
  if (block.kind === "thinking") {
    return (
      <details className="my-1 opacity-60">
        <summary className="cursor-pointer text-xs">thinking…</summary>
        <div className="text-xs pl-2 italic">
          {String(block.data.thinking ?? "")}
        </div>
      </details>
    );
  }
  if (block.kind === "tool_use") {
    return (
      <div className="my-1 text-xs opacity-70 italic">
        → tool: {String(block.data.name ?? "?")}
      </div>
    );
  }
  if (block.kind === "tool_result") {
    return (
      <details className="my-1 text-xs opacity-70">
        <summary className="cursor-pointer">tool result</summary>
        <pre className="pl-2 whitespace-pre-wrap text-xs">
          {typeof block.data.content === "string"
            ? block.data.content
            : JSON.stringify(block.data.content, null, 2)}
        </pre>
      </details>
    );
  }
  return (
    <div className="my-1 text-xs opacity-50 italic">[{block.kind}]</div>
  );
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Paperclip, X } from "lucide-react";
import { MessageBubble } from "@codeterm/provider-claude/src/MessageBubble";
import {
  parseJsonlSession,
  type ChatMessage,
} from "@codeterm/provider-claude/src/parser";
import { detectChatPrefix, type ChatOrigin } from "../../lib/chatPrefixes";
import { useMultiplexer } from "../../store/multiplexer";
import { quotePathForPaste } from "../../lib/clipboardPaste";
import { useTauriFileDrop } from "../../hooks/terminal";
import {
  filterSlashCommands,
  type SlashCommandSuggestion,
} from "../../lib/slashCommands";

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

function AttachmentChip({ path, onRemove }: { path: string; onRemove: () => void }) {
  const name = path.split(/[\\/]/).pop() || path;
  return (
    <div
      className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-mono"
      style={{
        background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
        color: "var(--ct-fg, #c9d1d9)",
        border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 35%, transparent)",
      }}
    >
      <Paperclip size={10} className="opacity-70" />
      <span className="max-w-[200px] truncate" title={path}>{name}</span>
      <button
        type="button"
        onClick={onRemove}
        className="opacity-60 hover:opacity-100"
        style={{ background: "transparent", border: "none", padding: 0, cursor: "pointer", color: "inherit" }}
        title="Remove"
      >
        <X size={10} />
      </button>
    </div>
  );
}

function SlashPicker({
  suggestions,
  highlighted,
  onPick,
}: {
  suggestions: SlashCommandSuggestion[];
  highlighted: number;
  onPick: (cmd: SlashCommandSuggestion) => void;
}) {
  if (suggestions.length === 0) return null;
  return (
    <div
      className="absolute bottom-full left-0 right-0 mb-1 max-h-48 overflow-y-auto rounded"
      style={{
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 90%, black 10%)",
        border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
        zIndex: 10,
      }}
    >
      {suggestions.map((c, i) => (
        <button
          key={c.name}
          type="button"
          onClick={() => onPick(c)}
          className="w-full flex items-center justify-between gap-2 px-2 py-1 text-xs font-mono text-left"
          style={{
            background: i === highlighted
              ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 20%, transparent)"
              : "transparent",
            color: "var(--ct-fg, #c9d1d9)",
            border: "none",
            cursor: "pointer",
          }}
        >
          <span style={{ color: "var(--ct-accent, #58a6ff)" }}>{c.name}</span>
          <span className="opacity-60 truncate">{c.description}</span>
        </button>
      ))}
    </div>
  );
}

export function PaneSessionChat({ paneId, onError }: Props) {
  const [rawText, setRawText] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const [attachments, setAttachments] = useState<string[]>([]);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // chatRootRef scopes the drag-drop zone to the entire chat pane.
  const chatRootRef = useRef<HTMLDivElement>(null);

  const suggestions = useMemo(
    () => filterSlashCommands(input.split("\n")[0] || ""),
    [input],
  );

  // Open picker when first line starts with / and there are matches.
  useEffect(() => {
    const firstLine = input.split("\n")[0] || "";
    setPickerOpen(firstLine.startsWith("/") && suggestions.length > 0);
    setPickerHighlight(0);
  }, [input, suggestions.length]);

  // Append paths to the attachment list, de-duping.
  const addAttachments = useCallback((paths: string[]) => {
    if (paths.length === 0) return;
    setAttachments((prev) => {
      const seen = new Set(prev);
      const next = [...prev];
      for (const p of paths) {
        if (!seen.has(p)) {
          seen.add(p);
          next.push(p);
        }
      }
      return next;
    });
  }, []);

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

  // Focus the composer when chat opens.
  useEffect(() => {
    const id = setTimeout(() => textareaRef.current?.focus(), 0);
    return () => clearTimeout(id);
  }, []);

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

  const { state: muxState, setFocusedPane } = useMultiplexer();

  const enrichedMessages = useMemo(() => {
    return allMessages.map((m) => {
      if (m.type !== "user") return { msg: m, origin: undefined as ChatOrigin | undefined };
      const prefix = detectChatPrefix(m.content);
      if (!prefix) return { msg: m, origin: undefined };
      return {
        msg: { ...m, content: prefix.body },
        origin: prefix.origin,
      };
    });
  }, [allMessages]);

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

  // Bug 2: drag-drop scoped to the entire chat pane (chatRootRef).
  useTauriFileDrop({
    containerRef: chatRootRef as React.RefObject<HTMLElement | null>,
    enabled: !loading && !error,
    onPaths: (paths) => addAttachments(paths),
  });

  async function send() {
    const text = input.trim();
    if ((!text && attachments.length === 0) || sending) return;
    setSending(true);
    // Compose: text + each quoted path separated by spaces — same pipeline
    // as useTauriFileDrop pasting into xterm.
    const pathLines = attachments.map(quotePathForPaste).join(" ");
    const composed = text && pathLines
      ? `${text} ${pathLines}`
      : text || pathLines;
    setOptimistic((prev) => [
      ...prev,
      {
        id: `opt-${Date.now()}`,
        type: "user",
        content: composed,
        // `timestamp` doubles as the optimistic-sentAt clock — we read
        // it back as a number for TTL pruning. ChatMessage.timestamp is
        // typed `string` so we stringify; the millis fit comfortably.
        timestamp: String(Date.now()),
      },
    ]);
    setInput("");
    setAttachments([]);
    try {
      await invoke("chat_send_text", { paneId, text: composed, enter: true });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
      // Restore focus — user expects to keep typing without clicking back.
      textareaRef.current?.focus();
    }
  }

  // Bug 3: shared paste handler callable from both textarea onPaste and
  // the window-level listener. Uses plain ClipboardEvent (not React's).
  const handlePasteEvent = useCallback(async (e: ClipboardEvent) => {
    const cd = e.clipboardData;
    if (!cd) return;

    // 1. OS-native file references (Ctrl+V on a copied file in Explorer/Finder)
    //    via the Tauri command that bridges CF_HDROP / NSPasteboardTypeFileURL.
    try {
      const paths = await invoke<string[]>("read_clipboard_files");
      if (paths && paths.length > 0) {
        e.preventDefault();
        addAttachments(paths);
        return;
      }
    } catch { /* fall through to image / text */ }

    // 2. Image data (screenshots, "copy image" from browser).
    const imageItem = Array.from(cd.items).find(
      (it) => it.kind === "file" && it.type.startsWith("image/"),
    );
    if (imageItem) {
      // Prevent default synchronously before any async work.
      e.preventDefault();
      const file = imageItem.getAsFile();
      if (!file) return;
      const reader = new FileReader();
      reader.onloadend = async () => {
        const result = reader.result;
        if (typeof result !== "string") return;
        // result = "data:image/png;base64,iVBOR..." — strip the data: prefix.
        const commaIdx = result.indexOf(",");
        const b64 = commaIdx >= 0 ? result.slice(commaIdx + 1) : result;
        const mime = file.type || "image/png";
        const suffix = mime.split("/")[1] || "png";
        try {
          const path = await invoke<string>("save_temp_file", {
            data: b64,
            extension: suffix,
          });
          addAttachments([path]);
        } catch (err) {
          console.warn("[chat] paste-image failed:", err);
        }
      };
      reader.readAsDataURL(file);
      return;
    }

    // 3. Otherwise — let the textarea's default paste behaviour handle text.
  }, [addAttachments]);

  // Bug 3: window-level paste listener so Ctrl+V works without textarea focus.
  // Only fires when the active element is inside this chat root.
  useEffect(() => {
    const onWindowPaste = (e: ClipboardEvent) => {
      if (
        chatRootRef.current &&
        !chatRootRef.current.contains(document.activeElement)
      ) {
        return;
      }
      void handlePasteEvent(e);
    };
    window.addEventListener("paste", onWindowPaste);
    return () => window.removeEventListener("paste", onWindowPaste);
  }, [handlePasteEvent]);

  function pickSuggestion(cmd: SlashCommandSuggestion) {
    const lines = input.split("\n");
    lines[0] = cmd.name + " ";
    setInput(lines.join("\n"));
    setPickerOpen(false);
    textareaRef.current?.focus();
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
      ref={chatRootRef}
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
        {enrichedMessages.length === 0 && (
          <div className="opacity-50 text-center py-8 text-sm">
            No messages yet.
          </div>
        )}
        {enrichedMessages.map(({ msg, origin }) => {
          const isPending = msg.id.startsWith("opt-");
          let onOriginClick: (() => void) | undefined;
          if (origin?.kind === "pane") {
            const idShort = origin.paneIdShort;
            const fullId = Object.keys(muxState.panes).find((id) => id.startsWith(idShort));
            if (fullId) onOriginClick = () => setFocusedPane(fullId);
          }
          return (
            <div
              key={msg.id}
              style={isPending ? { opacity: 0.55 } : undefined}
              title={isPending ? "sending… (echo expected within 60s)" : undefined}
            >
              <MessageBubble message={msg} origin={origin} onOriginClick={onOriginClick} />
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
        {attachments.length > 0 && (
          <div className="flex flex-wrap gap-1 mb-2">
            {attachments.map((p) => (
              <AttachmentChip
                key={p}
                path={p}
                onRemove={() =>
                  setAttachments((prev) => prev.filter((x) => x !== p))
                }
              />
            ))}
          </div>
        )}
        {/* Bug 4: relative wrapper so SlashPicker positions above textarea */}
        <div className="relative">
          <SlashPicker
            suggestions={pickerOpen ? suggestions : []}
            highlighted={pickerHighlight}
            onPick={pickSuggestion}
          />
          <textarea
            ref={textareaRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (pickerOpen) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setPickerHighlight((h) => Math.min(h + 1, suggestions.length - 1));
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setPickerHighlight((h) => Math.max(h - 1, 0));
                  return;
                }
                if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
                  e.preventDefault();
                  const cmd = suggestions[pickerHighlight];
                  if (cmd) pickSuggestion(cmd);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setPickerOpen(false);
                  return;
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
            onPaste={(e) => void handlePasteEvent(e.nativeEvent)}
            placeholder="Type a message…  (Enter to send, Shift+Enter for newline. Drag files or paste images.)"
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
    </div>
  );
}

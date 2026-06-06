import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getAgentProvider } from "../../lib/agentProviders";
import { useTauriFileDrop } from "../../hooks/terminal";
import { extractFilePathsFromClipboard } from "../../lib/clipboardPaste";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { MessageBubble } from "@codeterm/chat-ui";
import type { ChatMessage } from "@codeterm/provider-claude/src/parser";
import { AlertTriangle, File } from "lucide-react";
import { SendIcon, ChevronUpIcon } from "../Icons";

// TODO(api-chat): ChatView is reserved for future direct-API chat mode
// (pick a model, talk to LLMs via HTTP — no PTY in the middle).
// Terminal/JSONL parsing was removed — the view currently only renders
// user-sent optimistic messages + pending prompts. Wire the API path here
// when the feature lands; keep the input/attachments/prompt UI as the shell.

interface ChatViewProps {
  paneId: string;
  ptyId?: string | null;
  title?: string;
  agentType?: string | null;
  onToggle: () => void;
}

interface Attachment {
  id: string;
  name: string;
  path?: string;         // file system path for sending to PTY
  preview?: string;      // base64 data URL for image preview
  type: string;          // mime type
  _base64?: string;      // raw base64 for non-image files (pasted, no path)
}

interface PromptPayload {
  question: string;
  options: { label: string; value: string }[];
}

interface PaneSnapshot {
  id: string;
  pty_id?: string | null;
  cwd?: string | null;
  title?: string;
  agent_type?: string | null;
  agent_session_id?: string | null;
  pending_prompt?: PromptPayload | null;
}

interface MuxSnapshot {
  panes?: Record<string, PaneSnapshot>;
}

function basename(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]);
function mimeFromPath(p: string): string {
  const ext = p.split(".").pop()?.toLowerCase() || "";
  if (IMAGE_EXTS.has(ext)) return ext === "jpg" ? "image/jpeg" : `image/${ext}`;
  return "application/octet-stream";
}

export function ChatView({ paneId, ptyId, agentType, onToggle: _onToggle }: ChatViewProps) {
  const provider = getAgentProvider(agentType);
  const [allMessages, setAllMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = usePersistentDraft(`chat-view:${paneId}`);
  const [isLoading, _setIsLoading] = useState(false);
  const [visibleCount, setVisibleCount] = useState(50);
  const [waitingForReply, setWaitingForReply] = useState(false);
  const [inputExpanded, setInputExpanded] = useState(false);
  const [inputAtMax, setInputAtMax] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [pendingPrompt, setPendingPrompt] = useState<PromptPayload | null>(null);
  const pendingPromptRef = useRef(pendingPrompt);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  useTauriFileDrop({
    containerRef: rootRef,
    onPaths: (paths) => {
      setAttachments(prev => [
        ...prev,
        ...paths.map((path) => ({
          id: `drop-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          name: basename(path),
          path,
          type: mimeFromPath(path),
        })),
      ]);
      setDragOver(false);
      textareaRef.current?.focus();
    },
  });

  // Clear waitingForReply when Claude responds
  useEffect(() => {
    if (waitingForReply && allMessages.length > 0) {
      const last = allMessages[allMessages.length - 1];
      if (last && last.type !== 'user') {
        setWaitingForReply(false);
      }
    }
  }, [allMessages, waitingForReply]);

  // Show only last N messages for performance
  const messages = allMessages.length > visibleCount
    ? allMessages.slice(allMessages.length - visibleCount)
    : allMessages;
  const hasMore = allMessages.length > visibleCount;

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const userScrolledUp = useRef(false);

  // Prompt subscription — event-driven, no polling. Kept so permission
  // prompts still surface in the chat view (approve/deny dialogs).
  useEffect(() => {
    let active = true;

    (async () => {
      try {
        const state = await invoke<MuxSnapshot>("mux_get_state");
        if (!active) return;
        const prompt = state?.panes?.[paneId]?.pending_prompt ?? null;
        if (prompt) {
          setPendingPrompt(prompt);
          pendingPromptRef.current = prompt;
        }
      } catch { /* ignore */ }
    })();

    const unlistenPrompt = listen<{ pane_id: string; prompt: PromptPayload | null }>("prompt-state-changed", ({ payload }) => {
      if (payload.pane_id === paneId) {
        const prompt = payload.prompt ?? null;
        const hadPrompt = pendingPromptRef.current != null;
        setPendingPrompt(prompt);
        pendingPromptRef.current = prompt;
        if (prompt && !hadPrompt) {
          requestAnimationFrame(() => {
            messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
          });
        }
        if (!prompt && hadPrompt) {
          setWaitingForReply(false);
        }
      }
    });

    return () => {
      active = false;
      unlistenPrompt.then(fn => fn());
    };
  }, [paneId]);

  // Track user scroll
  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    userScrolledUp.current = !atBottom;
  }, []);

  // Send input to PTY with optimistic UI
  const sendMessage = useCallback(async () => {
    const text = input.trim();
    const currentAttachments = [...attachments];
    if (!text && currentAttachments.length === 0) return;
    setInput("");
    setAttachments([]);
    setInputExpanded(false);
    setInputAtMax(false);
    if (textareaRef.current) textareaRef.current.style.height = "20px";

    // Optimistic: single user message with text + attachment indicators
    const attLabels = currentAttachments.map(a => `[${a.name}]`);
    const displayContent = [text, ...attLabels].filter(Boolean).join(" ");
    if (displayContent) {
      setAllMessages(prev => [...prev, {
        id: `optimistic-${Date.now()}`,
        type: 'user' as const,
        content: displayContent,
      }]);
    }
    setWaitingForReply(true);

    userScrolledUp.current = false;
    requestAnimationFrame(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    });

    try {
      const resolvePtyId = async (): Promise<string | null> => {
        if (ptyId) return ptyId;
        const state = await invoke<{ panes: Record<string, { pty_id: string | null }> }>("mux_get_state");
        return state.panes[paneId]?.pty_id || null;
      };
      const pid = await resolvePtyId();
      if (!pid) return;

      // Handle attachments — strategy depends on provider
      const imageStrategy = provider?.imageSend || { method: 'path' as const };
      const fileMentions: string[] = [];

      // Phase 1: clipboard-paste images (must happen before text)
      for (const att of currentAttachments) {
        let filePath = att.path;

        // Resolve file path: from disk, from image preview, or from raw base64
        if (!filePath) {
          const base64 = att.preview?.split(",")[1] || att._base64;
          if (base64) {
            try {
              const ext = att.type.split("/")[1] || "bin";
              filePath = await invoke<string>("save_temp_file", { data: base64, extension: ext, name: att.name });
            } catch { /* skip */ }
          }
        }
        if (!filePath) continue;

        if (att.type.startsWith("image/") && imageStrategy.method === 'clipboard-paste') {
          // Provider uses clipboard paste for images (e.g. Claude Code)
          try {
            await invoke("copy_image_to_clipboard", { path: filePath });
            await new Promise(r => setTimeout(r, 300));
            await invoke("write_pty_string", { ptyId: pid, data: imageStrategy.pasteKeystroke || "\x16" });
            await new Promise(r => setTimeout(r, 500));
          } catch (err) {
            // Clipboard paste failed — fall back to sending the file path
            console.warn("[ChatView] clipboard paste failed, using path fallback:", err);
            fileMentions.push(filePath);
          }
        } else {
          // Non-image files: collect paths to include in text message
          fileMentions.push(filePath);
        }
      }

      // Phase 2: send text + file paths as one message
      const parts: string[] = [];
      for (const fp of fileMentions) {
        parts.push(`[attached: ${fp}]`);
      }
      if (text) parts.push(text);

      if (parts.length > 0) {
        await invoke("write_pty_string", { ptyId: pid, data: parts.join(" ") + "\r" });
      } else if (currentAttachments.length > 0) {
        // All were clipboard-pasted images, just send Enter
        await invoke("write_pty_string", { ptyId: pid, data: "\r" });
      }
    } catch {
      // PTY might be dead
    }
  }, [input, attachments, ptyId, paneId]);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const items = Array.from(e.clipboardData.items);
    const fileItems = items.filter(item => item.kind === "file");

    // Real filesystem paths from the clipboard ALWAYS win over getAsFile() blobs.
    // macOS Cmd+C of a Finder file puts a file URL on the pasteboard but
    // getAsFile() yields a generic 1024x1024 PNG icon, not the file's bytes.
    // The same is true for some Windows Explorer file copies.
    const pathsFromClipboard = extractFilePathsFromClipboard(e.clipboardData);

    if (pathsFromClipboard.length > 0) {
      e.preventDefault();
      const newAttachments: Attachment[] = pathsFromClipboard.map((p) => ({
        id: `paste-${Date.now()}-${Math.random().toString(36).slice(2, 5)}`,
        name: basename(p),
        path: p,
        type: mimeFromPath(p),
      }));
      setAttachments(prev => [...prev, ...newAttachments]);
      return;
    }

    if (fileItems.length > 0) {
      // No path on the clipboard — these are real in-memory bytes
      // (e.g. Shift+Cmd+Ctrl+4 screenshot, or a "copy image" from a webpage).
      e.preventDefault();
      for (const item of fileItems) {
        const blob = item.getAsFile();
        if (!blob) {
          console.warn("[ChatView] paste item has no file blob:", item.type, item.kind);
          continue;
        }

        const isImage = item.type.startsWith("image/");
        const ext = item.type.split("/")[1] || "bin";
        const name = isImage ? `image.${ext}` : `file.${ext}`;

        const reader = new FileReader();
        reader.onload = () => {
          const result = reader.result as string;
          const entry: Attachment = {
            id: `paste-${Date.now()}-${Math.random().toString(36).slice(2, 5)}`,
            name,
            type: item.type,
            preview: isImage ? result : undefined,
            _base64: isImage ? undefined : result.split(",")[1],
          };
          setAttachments(prev => [...prev, entry]);
        };
        reader.readAsDataURL(blob);
      }
      return;
    }

    e.preventDefault();
    const text = e.clipboardData.getData("text/plain");
    if (!text) return;

    const el = e.target as HTMLTextAreaElement;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const newValue = input.substring(0, start) + text + input.substring(end);
    setInput(newValue);

    requestAnimationFrame(() => {
      el.selectionStart = el.selectionEnd = start + text.length;
      if (!inputExpanded) {
        el.style.height = "0";
        el.style.height = Math.min(el.scrollHeight, 120) + "px";
        setInputAtMax(el.scrollHeight >= 120);
      }
    });
  }, [input, inputExpanded]);

  const toggleExpand = useCallback(() => {
    setInputExpanded(prev => {
      if (prev) {
        // Collapsing — recalc height to fit content
        requestAnimationFrame(() => {
          const el = textareaRef.current;
          if (el) {
            el.style.height = "0";
            el.style.height = Math.min(el.scrollHeight, 120) + "px";
            setInputAtMax(el.scrollHeight >= 120);
          }
        });
      }
      return !prev;
    });
  }, []);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el || inputExpanded) return;
    el.style.height = "0";
    const newH = Math.min(el.scrollHeight, 120);
    el.style.height = newH + "px";
    setInputAtMax(newH >= 120);
  }, [input, inputExpanded]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        sendMessage();
      }
      // Cmd+Shift+E / Ctrl+Shift+E — toggle expand
      if (e.key === "e" && e.shiftKey && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        toggleExpand();
      }
    },
    [sendMessage],
  );

  return (
    <div
      ref={rootRef}
      className="flex flex-col w-full h-full select-text"
      style={{
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
        outline: dragOver ? "2px dashed var(--ct-blue, #58a6ff)" : undefined,
        outlineOffset: "-2px",
      }}
      onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
      onDragLeave={() => setDragOver(false)}
    >
      {/* Messages area */}
      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto py-4 px-4 select-text cursor-text"
      >
        {isLoading ? (
          <div className="flex flex-col items-center justify-center h-full gap-3">
            <div className="flex gap-1">
              <div className="w-2 h-2 rounded-full bg-[var(--ct-blue)] animate-bounce" style={{ animationDelay: '0ms' }} />
              <div className="w-2 h-2 rounded-full bg-[var(--ct-blue)] animate-bounce" style={{ animationDelay: '150ms' }} />
              <div className="w-2 h-2 rounded-full bg-[var(--ct-blue)] animate-bounce" style={{ animationDelay: '300ms' }} />
            </div>
            <span className="text-xs" style={{ color: "var(--ct-muted, #8b949e)" }}>
              Loading conversation...
            </span>
          </div>
        ) : messages.length === 0 && !pendingPrompt ? (
          <div className="flex items-center justify-center h-full">
            <span
              className="text-xs"
              style={{ color: "var(--ct-muted, #8b949e)" }}
            >
              No messages yet. Start typing below.
            </span>
          </div>
        ) : (
          <>
            {hasMore && (
              <div className="flex justify-center py-2">
                <button
                  onClick={() => setVisibleCount(prev => prev + 50)}
                  className="text-[10px] px-3 py-1 rounded-md transition-colors"
                  style={{ color: "var(--ct-blue)", background: "var(--ct-bg-hover, rgba(255,255,255,0.04))" }}
                >
                  Load {Math.min(50, allMessages.length - visibleCount)} more...
                </button>
              </div>
            )}
            {messages.map((msg) => <MessageBubble key={msg.id} message={msg} />)}
            {/* Thinking indicator — shown after sending message, before Claude responds */}
            {waitingForReply && !pendingPrompt && (
              <div className="mb-3 max-w-4xl mx-auto">
                <span
                  className="inline-flex items-center gap-1 text-sm"
                  style={{ color: "var(--ct-muted-fg, #8b949e)" }}
                >
                  <span className="inline-flex gap-0.5">
                    <span className="animate-bounce" style={{ animationDelay: '0ms', animationDuration: '1s' }}>.</span>
                    <span className="animate-bounce" style={{ animationDelay: '200ms', animationDuration: '1s' }}>.</span>
                    <span className="animate-bounce" style={{ animationDelay: '400ms', animationDuration: '1s' }}>.</span>
                  </span>
                </span>
              </div>
            )}
            {/* Permission prompt — interactive buttons */}
            {pendingPrompt && (
              <div className="mb-3 max-w-4xl mx-auto">
                <div
                  className="rounded-lg px-4 py-3 border"
                  style={{
                    background: "color-mix(in srgb, var(--ct-yellow) 5%, var(--ct-surface))",
                    borderColor: "color-mix(in srgb, var(--ct-yellow) 30%, transparent)",
                  }}
                >
                  <div className="flex items-center gap-2 mb-2">
                    <AlertTriangle size={14} strokeWidth={2} style={{ stroke: "var(--ct-yellow)" }} />
                    <span className="text-xs font-medium" style={{ color: "var(--ct-yellow)" }}>
                      {pendingPrompt.question}
                    </span>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {pendingPrompt.options.map((opt) => (
                      <button
                        key={opt.value}
                        onClick={async () => {
                          setPendingPrompt(null);
                          pendingPromptRef.current = null;
                          try {
                            await invoke("mux_respond_prompt", { paneId, value: opt.value });
                          } catch { /* ignore */ }
                        }}
                        className="px-3 py-1.5 rounded-md text-xs font-medium transition-colors hover:brightness-110"
                        style={{
                          background: opt.value === "1"
                            ? "color-mix(in srgb, var(--ct-green) 20%, transparent)"
                            : "var(--ct-surface)",
                          color: opt.value === "1" ? "var(--ct-green)" : "var(--ct-fg)",
                          border: "1px solid color-mix(in srgb, var(--ct-border) 50%, transparent)",
                        }}
                      >
                        {opt.label}
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Input area */}
      <div
        className="flex-shrink-0 px-3 py-2 transition-all duration-200"
        style={{
          borderTop: "1px solid var(--ct-border, #30363d)",
          background: "var(--ct-surface, #161b22)",
          maxHeight: inputExpanded ? "80vh" : undefined,
        }}
      >
        {/* Attachments preview */}
        {attachments.length > 0 && (
          <div className="flex gap-2 pb-2 pt-1 px-1 overflow-x-auto" style={{ overflow: "visible" }}>
            {attachments.map(att => (
              <div key={att.id} className="relative flex-shrink-0 group">
                {att.preview ? (
                  <img
                    src={att.preview}
                    alt={att.name}
                    className="h-16 w-auto rounded-md object-cover border"
                    style={{ borderColor: "var(--ct-border, #30363d)" }}
                  />
                ) : (
                  <div
                    className="h-16 px-3 flex items-center gap-1.5 rounded-md border text-xs"
                    style={{ borderColor: "var(--ct-border, #30363d)", background: "var(--ct-bg, #0d1117)", color: "var(--ct-muted-fg, #8b949e)" }}
                  >
                    <File size={14} strokeWidth={2} />
                    {att.name.length > 20 ? att.name.slice(0, 17) + "..." : att.name}
                  </div>
                )}
                <button
                  onClick={() => setAttachments(prev => prev.filter(a => a.id !== att.id))}
                  className="absolute -top-2 -right-2 w-5 h-5 rounded-full flex items-center justify-center text-[10px] leading-none font-bold shadow-md"
                  style={{ background: "#f85149", color: "#fff" }}
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        )}
        {/* Expand button — visible only when textarea hit max height */}
        {inputAtMax && !inputExpanded && (
          <button
            onClick={toggleExpand}
            className="w-full flex items-center justify-center gap-1 py-1 text-[10px] transition-colors hover:bg-[var(--ct-bg-hover)]"
            style={{ color: "var(--ct-muted, #8b949e)" }}
          >
            <ChevronUpIcon />
            Expand <span style={{ opacity: 0.5 }}>(⌘⇧E)</span>
          </button>
        )}
        {inputExpanded && (
          <button
            onClick={toggleExpand}
            className="w-full flex items-center justify-center gap-1 py-1 text-[10px] transition-colors hover:bg-[var(--ct-bg-hover)]"
            style={{ color: "var(--ct-muted, #8b949e)" }}
          >
            <ChevronUpIcon style={{ transform: "rotate(180deg)" }} />
            Collapse <span style={{ opacity: 0.5 }}>(⌘⇧E)</span>
          </button>
        )}
        <div
          className={`flex gap-2 rounded-lg px-3 py-2 ${inputExpanded ? "items-stretch" : "items-center"}`}
          style={{
            background: "var(--ct-bg, #0d1117)",
            border: "1px solid var(--ct-border, #30363d)",
            height: inputExpanded ? "calc(80vh - 40px)" : undefined,
          }}
        >
          <textarea
            ref={textareaRef}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              // Auto-resize on change
              const el = e.target;
              if (!inputExpanded) {
                el.style.height = "0";
                const newH = Math.min(el.scrollHeight, 120);
                el.style.height = newH + "px";
                setInputAtMax(newH >= 120);
              }
            }}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            placeholder="Send a message..."
            rows={1}
            className="flex-1 bg-transparent border-none outline-none resize-none text-sm leading-5 placeholder:text-[var(--ct-muted)]"
            style={{
              color: "var(--ct-fg, #c9d1d9)",
              fontFamily: "inherit",
              maxHeight: inputExpanded ? "none" : "120px",
              minHeight: "20px",
              height: inputExpanded ? "100%" : undefined,
              overflowY: inputExpanded ? "auto" : inputAtMax ? "auto" : "hidden",
            }}
          />
          <button
            onClick={() => {
              sendMessage();
              setInputExpanded(false);
              setInputAtMax(false);
              if (textareaRef.current) {
                textareaRef.current.style.height = "20px";
              }
            }}
            disabled={!input.trim() && attachments.length === 0}
            className={`flex items-center justify-center w-7 h-7 rounded-md transition-colors duration-150 flex-shrink-0 disabled:opacity-30 ${inputExpanded ? "self-end" : ""}`}
            style={{
              color: "var(--ct-blue, #58a6ff)",
            }}
            title="Send (Enter)"
          >
            <SendIcon />
          </button>
        </div>
      </div>
    </div>
  );
}

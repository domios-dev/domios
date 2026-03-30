import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseJsonlSession, parseTerminalOutput } from "./parser";
import { getAgentProvider, isProviderNoise } from "../../lib/agentProviders";
import { MessageBubble } from "./MessageBubble";
import type { ChatMessage } from "./parser";

interface ChatViewProps {
  paneId: string;
  ptyId?: string | null;
  title?: string;
  agentType?: string | null;
  onToggle: () => void;
}

type ParseMode = "jsonl" | "terminal" | "detecting";

export function ChatView({ paneId, ptyId, agentType, onToggle: _onToggle }: ChatViewProps) {
  const provider = getAgentProvider(agentType);
  const rawSessionParser = provider?.parseSessionData || parseJsonlSession;
  const rawTerminalParser = provider?.parseTerminalOutput || parseTerminalOutput;

  // Wrap parsers with noise filter from provider
  const filterNoise = useCallback((msgs: ChatMessage[]) =>
    msgs.filter(m => !isProviderNoise(agentType, m.content)),
  [agentType]);
  const sessionParser = useCallback((data: string) => filterNoise(rawSessionParser(data)), [rawSessionParser, filterNoise]);
  const terminalParser = useCallback((data: string) => filterNoise(rawTerminalParser(data)), [rawTerminalParser, filterNoise]);
  const [allMessages, setAllMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [visibleCount, setVisibleCount] = useState(50);
  const [waitingForReply, setWaitingForReply] = useState(false);
  const [inputExpanded, setInputExpanded] = useState(false);
  const [inputAtMax, setInputAtMax] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [pendingPrompt, setPendingPrompt] = useState<{
    question: string;
    options: { label: string; value: string }[];
  } | null>(null);
  const pendingPromptRef = useRef(pendingPrompt);
  const [attachments, setAttachments] = useState<Array<{
    id: string;
    name: string;
    path?: string;        // file system path for sending to PTY
    preview?: string;     // base64 data URL for image preview
    type: string;         // mime type
    _base64?: string;     // raw base64 for non-image files (pasted, no path)
  }>>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

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
  const prevMessageCount = useRef(0);
  const prevOutputLen = useRef(0);
  const jsonlOffset = useRef(0);
  const accumulatedJsonl = useRef("");
  const parseMode = useRef<ParseMode>("detecting");
  const jsonlRetryCount = useRef(0);

  // Poll output and parse
  useEffect(() => {
    let active = true;

    async function pollJsonl() {
      if (!active) return;
      try {
        const newData = await invoke<string>("read_session_jsonl", {
          paneId,
          offset: jsonlOffset.current,
        });
        if (!active) return;

        if (newData.length > 0) {
          jsonlOffset.current += new TextEncoder().encode(newData).length;
          accumulatedJsonl.current += newData;

          // Only re-parse when we got new data
          const parsed = sessionParser(accumulatedJsonl.current);
          if (parsed.length !== prevMessageCount.current) {
            // Keep optimistic messages until JSONL contains a matching user message
            setAllMessages(prev => {
              const optimistic = prev.filter(m => m.id.startsWith('optimistic-'));
              if (optimistic.length === 0) return parsed;
              // Check if JSONL now has a user message that matches each optimistic one
              const lastParsedUser = [...parsed].reverse().find(m => m.type === 'user');
              const lastOptimistic = optimistic[optimistic.length - 1];
              if (lastParsedUser && lastOptimistic &&
                  lastParsedUser.content.includes(lastOptimistic.content.slice(0, 40))) {
                // JSONL caught up — drop optimistic
                return parsed;
              }
              // JSONL hasn't caught up yet — keep optimistic at end
              return [...parsed, ...optimistic];
            });
            // Auto-scroll on new messages
            if (!userScrolledUp.current) {
              requestAnimationFrame(() => {
                messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
              });
            }
            prevMessageCount.current = parsed.length;
          }
        }
        // No new data — skip re-parse entirely
      } catch (err) {
        console.warn('[ChatView] JSONL poll error:', err);
        // JSONL not available — switch to terminal fallback
        if (parseMode.current === "detecting") {
          jsonlRetryCount.current++;
          // Give it a few tries before falling back (session might not be linked yet)
          if (jsonlRetryCount.current > 5) {
            parseMode.current = "terminal";
          }
        }
      }
    }

    async function pollTerminal() {
      if (!active) return;
      try {
        const text = await invoke<string>("get_pane_output", { paneId });
        if (!active) return;

        const parsed = terminalParser(text);
        setAllMessages(prev => {
          const optimistic = prev.filter(m => m.id.startsWith('optimistic-'));
          if (optimistic.length === 0) return parsed;
          // Check if terminal output now has the user message
          const lastParsedUser = [...parsed].reverse().find(m => m.type === 'user');
          const lastOptimistic = optimistic[optimistic.length - 1];
          if (lastParsedUser && lastOptimistic &&
              lastParsedUser.content.includes(lastOptimistic.content.slice(0, 40))) {
            return parsed;
          }
          return [...parsed, ...optimistic];
        });
        setIsLoading(false);

        const len = text.length;
        if (len > prevOutputLen.current && !userScrolledUp.current) {
          requestAnimationFrame(() => {
            messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
          });
        }
        prevOutputLen.current = len;
      } catch (err) {
        console.warn('[ChatView] terminal poll error:', err);
      }
    }

    // Read pending prompt from backend state (provider-agnostic)
    async function checkForPrompt() {
      try {
        const state = await invoke<any>("mux_get_state");
        if (!active) return;
        const pane = state?.panes?.[paneId];
        const prompt = pane?.pending_prompt ?? null;
        // Only update if changed
        const prev = pendingPromptRef.current;
        if (JSON.stringify(prompt) !== JSON.stringify(prev)) {
          setPendingPrompt(prompt);
          pendingPromptRef.current = prompt;
          // Auto-scroll to prompt when it appears
          if (prompt && !prev) {
            requestAnimationFrame(() => {
              messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
            });
          }
        }
      } catch { /* ignore */ }
    }

    let polling = false;
    async function poll() {
      if (!active || polling) return;
      polling = true;
      try { await doPoll(); } finally { polling = false; }
    }
    async function doPoll() {
      // Check for permission prompts from backend
      checkForPrompt();

      // On first poll or while detecting, try JSONL
      if (parseMode.current === "detecting") {
        // Show loading immediately, don't block
        setIsLoading(true);

        try {
          const data = await invoke<string>("read_session_jsonl", {
            paneId,
            offset: 0,
            tailBytes: 5000000,  // 5MB tail for fast initial load (JSONL lines can be large)
          });
          if (!active) return;
          // JSONL works — use it
          parseMode.current = "jsonl";

          // Extract offset marker from tail read
          let jsonlData = data;
          if (data.startsWith("__offset:")) {
            const nlIdx = data.indexOf("\n");
            const fileLen = parseInt(data.substring(9, nlIdx), 10);
            jsonlData = data.substring(nlIdx + 1);
            jsonlOffset.current = fileLen; // next poll reads from end of file
          } else {
            jsonlOffset.current = new TextEncoder().encode(data).length;
          }
          accumulatedJsonl.current = jsonlData;

          // Parse in next frame to avoid blocking UI
          requestAnimationFrame(() => {
            if (!active) return;
            const parsed = sessionParser(jsonlData);
            setAllMessages(parsed);
            setIsLoading(false);
            prevMessageCount.current = parsed.length;
            if (!userScrolledUp.current && parsed.length > 0) {
              requestAnimationFrame(() => {
                messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
              });
            }
          });
          return;
        } catch {
          jsonlRetryCount.current++;
          if (jsonlRetryCount.current > 3) {
            // JSONL not available — switch to terminal mode permanently
            // (will try to upgrade back every 10 polls)
            parseMode.current = "terminal";
            setIsLoading(false);
            await pollTerminal();
          }
          // While still detecting (< 3 retries), just show loading — don't flash terminal
          return;
        }
      }

      // In terminal mode, try to upgrade to JSONL every 5 seconds (every 10 polls at 500ms)
      if (parseMode.current === "terminal") {
        jsonlRetryCount.current++;
        if (jsonlRetryCount.current % 10 === 0) {
          try {
            const data = await invoke<string>("read_session_jsonl", { paneId, offset: 0, tailBytes: 5000000 });
            if (data && data.length > 50) {
              // JSONL is available now — switch!
              parseMode.current = "jsonl";
              let jsonlData = data;
              if (data.startsWith("__offset:")) {
                const nlIdx = data.indexOf("\n");
                jsonlOffset.current = parseInt(data.substring(9, nlIdx), 10);
                jsonlData = data.substring(nlIdx + 1);
              } else {
                jsonlOffset.current = new TextEncoder().encode(data).length;
              }
              accumulatedJsonl.current = jsonlData;
              const parsed = sessionParser(jsonlData);
              setAllMessages(parsed);
              prevMessageCount.current = parsed.length;
              if (!userScrolledUp.current && parsed.length > 0) {
                requestAnimationFrame(() => {
                  messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
                });
              }
            }
          } catch { /* still no JSONL */ }
        }
      }

      if (parseMode.current === "jsonl") {
        await pollJsonl();
      } else {
        await pollTerminal();
      }
    }

    // Defer first poll so overlay renders instantly with loading state
    setTimeout(poll, 0);

    // Listen for output change events from backend — debounced to prevent flicker
    let eventPollTimer: ReturnType<typeof setTimeout> | null = null;
    const unlisten = listen<{ pty_id: string }>("pane-output-changed", () => {
      if (eventPollTimer) clearTimeout(eventPollTimer);
      eventPollTimer = setTimeout(poll, 300);
    });

    // Listen for mux state changes — picks up prompt detection faster
    const unlistenState = listen("mux-state-changed", () => {
      checkForPrompt();
    });

    // Fallback polling at 2s (event-driven handles the fast path)
    const interval = setInterval(poll, 2000);
    return () => {
      active = false;
      clearInterval(interval);
      unlisten.then(fn => fn());
      unlistenState.then(fn => fn());
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
      prevMessageCount.current += 1;
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
            await new Promise(r => setTimeout(r, 200));
            await invoke("write_pty_string", { ptyId: pid, data: imageStrategy.pasteKeystroke || "\x16" });
            await new Promise(r => setTimeout(r, 500));
          } catch { /* skip */ }
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

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDragOver(false);

    const files = Array.from(e.dataTransfer.files);
    for (const file of files) {
      const path = (file as unknown as { path?: string }).path;
      const id = `attach-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;

      const isImage = file.type.startsWith("image/");
      if (path) {
        // Dropped from Finder — has file path
        setAttachments(prev => [...prev, {
          id,
          name: file.name,
          path,
          preview: isImage ? undefined : undefined, // load preview below for images
          type: file.type,
        }]);
        if (isImage) {
          const reader = new FileReader();
          reader.onload = () => {
            setAttachments(prev => prev.map(a => a.id === id ? { ...a, preview: reader.result as string } : a));
          };
          reader.readAsDataURL(file);
        }
      } else {
        // No path — read as base64
        const reader = new FileReader();
        reader.onload = () => {
          const dataUrl = reader.result as string;
          const base64 = dataUrl.split(",")[1];
          setAttachments(prev => [...prev, {
            id,
            name: file.name,
            preview: isImage ? dataUrl : undefined,
            type: file.type,
            _base64: isImage ? undefined : base64,
          }]);
        };
        reader.readAsDataURL(file);
      }
    }
    // Focus input after drop
    textareaRef.current?.focus();
  }, []);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    // Check for files (images, PDFs, any file type)
    const items = Array.from(e.clipboardData.items);
    const fileItems = items.filter(item => item.kind === "file");

    if (fileItems.length > 0) {
      e.preventDefault();
      for (const item of fileItems) {
        const blob = item.getAsFile();
        if (!blob) continue;

        const isImage = item.type.startsWith("image/");
        const ext = item.type.split("/")[1] || "bin";
        const name = isImage ? `image.${ext}` : `file.${ext}`;

        const reader = new FileReader();
        reader.onload = () => {
          setAttachments(prev => [...prev, {
            id: `paste-${Date.now()}-${Math.random().toString(36).slice(2, 5)}`,
            name,
            preview: isImage ? (reader.result as string) : undefined,
            type: item.type,
            // Store base64 for non-image files too (for save_temp_file)
            ...(isImage ? {} : { _base64: (reader.result as string).split(",")[1] }),
          } as any]);
        };
        reader.readAsDataURL(blob);
      }
      return;
    }

    // Text paste
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
      className="flex flex-col w-full h-full select-text"
      style={{
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
        outline: dragOver ? "2px dashed var(--ct-blue, #58a6ff)" : undefined,
        outlineOffset: "-2px",
      }}
      onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleDrop}
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
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--ct-yellow)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M12 9v4"/><path d="M12 17h.01"/><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
                    </svg>
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
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>
                    </svg>
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
            <svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor">
              <path d="M3.22 9.78a.75.75 0 010-1.06l4.25-4.25a.75.75 0 011.06 0l4.25 4.25a.75.75 0 01-1.06 1.06L8 6.06 4.28 9.78a.75.75 0 01-1.06 0z" />
            </svg>
            Expand <span style={{ opacity: 0.5 }}>(⌘⇧E)</span>
          </button>
        )}
        {inputExpanded && (
          <button
            onClick={toggleExpand}
            className="w-full flex items-center justify-center gap-1 py-1 text-[10px] transition-colors hover:bg-[var(--ct-bg-hover)]"
            style={{ color: "var(--ct-muted, #8b949e)" }}
          >
            <svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor" style={{ transform: "rotate(180deg)" }}>
              <path d="M3.22 9.78a.75.75 0 010-1.06l4.25-4.25a.75.75 0 011.06 0l4.25 4.25a.75.75 0 01-1.06 1.06L8 6.06 4.28 9.78a.75.75 0 01-1.06 0z" />
            </svg>
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
            <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
              <path d="M.989 8 .064 2.68a1.342 1.342 0 011.85-1.462l13.402 5.744a1.13 1.13 0 010 2.076L1.913 14.782a1.343 1.343 0 01-1.85-1.463L.99 8Zm1.015.78l-.616 3.559a.14.14 0 00.193.153l10.24-4.387-9.817.674Zm-.616-5.119.616 3.559 9.817.674-10.24-4.387a.14.14 0 00-.193.154Z" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

export function ChatView({ paneId, ptyId, title, agentType, onToggle }: ChatViewProps) {
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
  const [isTyping, setIsTyping] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [visibleCount, setVisibleCount] = useState(50);
  const [waitingForReply, setWaitingForReply] = useState(false);
  const [inputExpanded, setInputExpanded] = useState(false);
  const [inputAtMax, setInputAtMax] = useState(false);
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
          setIsTyping(true);
        } else {
          setIsTyping(false);
        }

        const parsed = sessionParser(accumulatedJsonl.current);
        if (parsed.length !== prevMessageCount.current) {
          // Preserve optimistic messages not yet in JSONL
          setAllMessages(prev => {
            const optimistic = prev.filter(m => m.id.startsWith('optimistic-'));
            if (optimistic.length === 0) return parsed;
            // Check if optimistic content appeared in parsed (by matching text)
            const remaining = optimistic.filter(opt =>
              !parsed.some(p => p.type === 'user' && p.content.trim() === opt.content.trim())
            );
            return remaining.length > 0 ? [...parsed, ...remaining] : parsed;
          });
          // Auto-scroll on new messages
          if (!userScrolledUp.current) {
            requestAnimationFrame(() => {
              messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
            });
          }
          prevMessageCount.current = parsed.length;
        } else if (newData.length === 0) {
          setIsTyping(false);
        }
      } catch {
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
          const remaining = optimistic.filter(opt =>
            !parsed.some(p => p.type === 'user' && p.content.trim() === opt.content.trim())
          );
          return remaining.length > 0 ? [...parsed, ...remaining] : parsed;
        });
        setIsLoading(false);

        const len = text.length;
        setIsTyping(len > prevOutputLen.current);
        if (len > prevOutputLen.current && !userScrolledUp.current) {
          requestAnimationFrame(() => {
            messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
          });
        }
        prevOutputLen.current = len;
      } catch {
        // Pane might not exist yet
      }
    }

    async function poll() {
      if (!active) return;

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
          if (jsonlRetryCount.current > 5) {
            parseMode.current = "terminal";
            setIsLoading(false);
          }
          // Fall through to terminal for this tick
          await pollTerminal();
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
    // Poll frequently for responsive streaming
    const interval = setInterval(poll, 500);
    return () => {
      active = false;
      clearInterval(interval);
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
    if (!text) return;
    setInput("");
    setInputExpanded(false);
    setInputAtMax(false);
    if (textareaRef.current) textareaRef.current.style.height = "20px";

    // Optimistic: show user message immediately
    const optimisticMsg: ChatMessage = {
      id: `optimistic-${Date.now()}`,
      type: 'user',
      content: text,
    };
    setAllMessages(prev => [...prev, optimisticMsg]);
    prevMessageCount.current += 1;
    setWaitingForReply(true);

    userScrolledUp.current = false;
    requestAnimationFrame(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    });

    try {
      // Use pane-level send_text which resolves pty internally
      if (ptyId) {
        await invoke("write_pty_string", { ptyId, data: text + "\r" });
      } else {
        // Fallback: use service-level send via REST-like command
        const panes: any[] = await invoke("mux_list_panes");
        const pane = panes.find((p: any) => p.id === paneId);
        if (pane?.pty_id) {
          await invoke("write_pty_string", { ptyId: pane.pty_id, data: text + "\r" });
        }
      }
    } catch {
      // PTY might be dead
    }
  }, [input, ptyId, paneId]);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
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
      // Only resize in contracted mode
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
      }}
    >
      {/* Header */}
      <div
        className="flex items-center justify-between px-3 py-2 flex-shrink-0"
        style={{
          borderBottom: "1px solid var(--ct-border, #30363d)",
          background: "var(--ct-surface, #161b22)",
        }}
      >
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-xs font-medium truncate opacity-80">
            {title || "Claude"}
          </span>
          {isTyping && (
            <span className="text-[10px] text-[var(--ct-blue)] animate-pulse">
              thinking...
            </span>
          )}
        </div>
        <button
          onClick={onToggle}
          className="flex items-center gap-1.5 px-2 py-1 rounded-md text-[10px] font-medium transition-colors duration-150 hover:bg-[var(--ct-bg-hover)]"
          style={{ color: "var(--ct-muted, #8b949e)" }}
          title="Switch to terminal view"
        >
          <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
            <path d="M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0114.25 15H1.75A1.75 1.75 0 010 13.25Zm1.75-.25a.25.25 0 00-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 00.25-.25V2.75a.25.25 0 00-.25-.25ZM7.25 8a.75.75 0 01-.22.53l-2.25 2.25a.75.75 0 11-1.06-1.06L5.44 8 3.72 6.28a.75.75 0 111.06-1.06l2.25 2.25c.141.14.22.331.22.53Zm1.5 1.5a.75.75 0 000 1.5h3a.75.75 0 000-1.5Z" />
          </svg>
          Terminal
        </button>
      </div>

      {/* Messages area */}
      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto py-4 select-text cursor-text"
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
        ) : messages.length === 0 ? (
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
            {waitingForReply && (
              <div className="flex justify-start mb-3 px-4">
                <div
                  className="rounded-2xl rounded-bl-md px-4 py-2.5 text-sm"
                  style={{ background: "var(--ct-surface, #161b22)", color: "var(--ct-muted, #8b949e)" }}
                >
                  <span className="inline-flex gap-1">
                    <span className="animate-bounce" style={{ animationDelay: '0ms', animationDuration: '1s' }}>.</span>
                    <span className="animate-bounce" style={{ animationDelay: '200ms', animationDuration: '1s' }}>.</span>
                    <span className="animate-bounce" style={{ animationDelay: '400ms', animationDuration: '1s' }}>.</span>
                  </span>
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
            disabled={!input.trim()}
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

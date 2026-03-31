import { useState, useRef, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface FloatingInputProps {
  ptyId: string | null;
  agentType: string | null;
  paneId: string;
  pendingPrompt: {
    question: string;
    options: { label: string; value: string }[];
  } | null;
}

export default function FloatingInput({ ptyId, agentType, paneId, pendingPrompt }: FloatingInputProps) {
  const [expanded, setExpanded] = useState(false);
  const [input, setInput] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-expand when there's a pending prompt
  useEffect(() => {
    if (pendingPrompt) setExpanded(true);
  }, [pendingPrompt]);

  // Focus input when expanded
  useEffect(() => {
    if (expanded && inputRef.current) {
      inputRef.current.focus();
    }
  }, [expanded]);

  const handleSend = useCallback(async () => {
    if (!input.trim() || !ptyId) return;
    const text = input.trim();
    setInput("");
    try {
      await invoke("write_pty_string", { ptyId, data: text + "\r" });
    } catch { /* PTY might be dead */ }
  }, [input, ptyId]);

  const handlePromptResponse = useCallback(async (value: string) => {
    try {
      await invoke("mux_respond_prompt", { paneId, value });
    } catch { /* ignore */ }
  }, [paneId]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    if (e.key === "Escape") {
      setExpanded(false);
      setInput("");
    }
  }, [handleSend]);

  // Don't show if no agent running
  if (!agentType) return null;

  return (
    <div className="absolute bottom-3 right-3 z-20 flex flex-col items-end gap-2">
      {/* Pending prompt */}
      {pendingPrompt && (
        <div
          className="rounded-lg px-3 py-2 border max-w-sm shadow-lg"
          style={{
            background: "color-mix(in srgb, var(--ct-yellow) 8%, var(--ct-surface))",
            borderColor: "color-mix(in srgb, var(--ct-yellow) 30%, transparent)",
          }}
        >
          <div className="text-[10px] font-medium mb-1.5" style={{ color: "var(--ct-yellow)" }}>
            {pendingPrompt.question}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {pendingPrompt.options.map((opt) => (
              <button
                key={opt.value}
                onClick={() => handlePromptResponse(opt.value)}
                className="px-2 py-1 rounded text-[10px] font-medium transition-colors hover:brightness-110"
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
      )}

      {/* Input bubble */}
      {expanded ? (
        <div
          className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 border shadow-lg"
          style={{
            background: "var(--ct-surface)",
            borderColor: "var(--ct-border)",
            minWidth: "280px",
            maxWidth: "400px",
          }}
        >
          <input
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            onBlur={() => { if (!input) setExpanded(false); }}
            placeholder="Message agent..."
            className="flex-1 bg-transparent text-xs outline-none"
            style={{ color: "var(--ct-fg)" }}
          />
          <button
            onClick={handleSend}
            disabled={!input.trim()}
            className="p-1 rounded transition-colors"
            style={{
              color: input.trim() ? "var(--ct-blue)" : "var(--ct-muted)",
            }}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M22 2L11 13" /><path d="M22 2L15 22L11 13L2 9L22 2Z" />
            </svg>
          </button>
        </div>
      ) : (
        <button
          onClick={() => setExpanded(true)}
          className="w-8 h-8 rounded-full flex items-center justify-center shadow-lg transition-transform hover:scale-110"
          style={{
            background: "var(--ct-blue)",
            color: "#fff",
          }}
          title="Send message to agent"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          </svg>
        </button>
      )}
    </div>
  );
}

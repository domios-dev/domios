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

export default function FloatingInput({ ptyId, agentType, pendingPrompt }: FloatingInputProps) {
  const [visible, setVisible] = useState(false);
  const [input, setInput] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [inputExpanded, setInputExpanded] = useState(false);

  // Hide chat when prompt appears (so terminal prompt is visible)
  useEffect(() => {
    if (pendingPrompt) setVisible(false);
  }, [pendingPrompt]);

  useEffect(() => {
    if (visible && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [visible]);

  const handleSend = useCallback(async () => {
    if (!input.trim() || !ptyId) return;
    const text = input.trim();
    setInput("");
    // Reset textarea height
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
    try {
      await invoke("write_pty_string", { ptyId, data: text + "\r" });
    } catch { /* PTY might be dead */ }
  }, [input, ptyId]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    if (e.key === "Escape") {
      setVisible(false);
      setInput("");
    }
  }, [handleSend]);

  const handleInput = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInput(e.target.value);
    // Auto-resize
    const el = e.target;
    if (!inputExpanded) {
      el.style.height = "0";
      el.style.height = Math.min(el.scrollHeight, 120) + "px";
    }
  }, [inputExpanded]);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    // File paste (images etc)
    const items = Array.from(e.clipboardData.items);
    const fileItems = items.filter(item => item.kind === "file");
    if (fileItems.length > 0 && ptyId) {
      e.preventDefault();
      for (const item of fileItems) {
        const blob = item.getAsFile();
        if (!blob) continue;
        const reader = new FileReader();
        reader.onload = async () => {
          const base64 = (reader.result as string).split(",")[1];
          if (base64) {
            try {
              const ext = item.type.split("/")[1] || "bin";
              const path = await invoke<string>("save_temp_file", { data: base64, extension: ext, name: blob.name });
              // Send file path to agent
              await invoke("write_pty_string", { ptyId, data: path + "\r" });
            } catch { /* skip */ }
          }
        };
        reader.readAsDataURL(blob);
      }
      return;
    }

    // File path paste (Ctrl+C file in Explorer)
    const text = e.clipboardData.getData("text/plain");
    if (text) {
      const trimmed = text.trim();
      const looksLikeFile = /^([A-Za-z]:\\|\/|~\/)/.test(trimmed) && !trimmed.includes("\n") && trimmed.length < 500;
      if (looksLikeFile && ptyId) {
        e.preventDefault();
        invoke("write_pty_string", { ptyId, data: trimmed + "\r" }).catch(() => {});
        return;
      }
    }
    // Regular text paste — let default handler work
  }, [ptyId]);

  if (!agentType) return null;

  return (
    <div className="absolute bottom-3 left-1/2 -translate-x-1/2 z-20 flex flex-col items-center gap-2">
      {visible ? (
        <div
          className="rounded-xl border shadow-xl backdrop-blur-sm"
          style={{
            background: "color-mix(in srgb, var(--ct-surface) 95%, transparent)",
            borderColor: "var(--ct-border)",
            width: inputExpanded ? "min(90vw, 600px)" : "min(70vw, 420px)",
            transition: "width 0.2s ease",
          }}
        >
          <div className="flex items-start gap-2 p-2">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={handleInput}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              placeholder="Message agent..."
              rows={1}
              className="flex-1 bg-transparent text-xs outline-none resize-none leading-relaxed"
              style={{
                color: "var(--ct-fg)",
                minHeight: "24px",
                maxHeight: inputExpanded ? "300px" : "120px",
              }}
            />
            <div className="flex items-center gap-1 shrink-0 pt-0.5">
              {/* Expand toggle */}
              <button
                onClick={() => setInputExpanded(v => !v)}
                className="p-1 rounded transition-colors"
                style={{ color: "var(--ct-muted)" }}
                title={inputExpanded ? "Collapse" : "Expand"}
              >
                <svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor">
                  {inputExpanded ? (
                    <path d="M5.5 13.5L.5 8.5l5-5m5 0l5 5-5 5" stroke="currentColor" fill="none" strokeWidth="1.5"/>
                  ) : (
                    <path d="M.5 5.5l5-5 5 5m-10 5l5 5 5-5" stroke="currentColor" fill="none" strokeWidth="1.5"/>
                  )}
                </svg>
              </button>
              {/* Send */}
              <button
                onClick={handleSend}
                disabled={!input.trim()}
                className="p-1 rounded transition-colors"
                style={{ color: input.trim() ? "var(--ct-blue)" : "var(--ct-muted)" }}
                title="Send (Enter)"
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M22 2L11 13"/><path d="M22 2L15 22L11 13L2 9L22 2Z"/>
                </svg>
              </button>
              {/* Close */}
              <button
                onClick={() => { setVisible(false); setInput(""); }}
                className="p-1 rounded transition-colors"
                style={{ color: "var(--ct-muted)" }}
                title="Close (Esc)"
              >
                <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                  <path d="M2 2l8 8M10 2l-8 8"/>
                </svg>
              </button>
            </div>
          </div>
        </div>
      ) : (
        <button
          onClick={() => setVisible(true)}
          className="w-8 h-8 rounded-full flex items-center justify-center shadow-lg transition-all hover:scale-110 hover:shadow-xl"
          style={{
            background: "color-mix(in srgb, var(--ct-surface) 90%, transparent)",
            border: "1px solid var(--ct-border)",
            color: "var(--ct-muted-fg)",
          }}
          title="Chat with agent (click or start typing)"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
          </svg>
        </button>
      )}
    </div>
  );
}

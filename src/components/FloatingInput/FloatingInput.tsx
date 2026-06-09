import { useState, useRef, useCallback, useEffect } from "react";
import { invoke } from "../../lib/bridge";
import { X, MessageSquare } from "lucide-react";

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
  const [lastSent, setLastSent] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Hide chat when prompt appears
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
    setLastSent(text);
    setInput("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
    try {
      await invoke("write_pty_string", { ptyId, data: text + "\r", raw: false });
    } catch { /* PTY might be dead */ }
  }, [input, ptyId]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    if (e.key === "Escape") {
      if (input.trim()) setLastSent(input.trim());
      setVisible(false);
      setInput("");
    }
  }, [handleSend]);

  const handleInput = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInput(e.target.value);
    const el = e.target;
    el.style.height = "0";
    el.style.height = Math.min(el.scrollHeight, 200) + "px";
  }, []);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
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
              await invoke("write_pty_string", { ptyId, data: path + "\r", raw: false });
            } catch { /* skip */ }
          }
        };
        reader.readAsDataURL(blob);
      }
      return;
    }
    const text = e.clipboardData.getData("text/plain");
    if (text) {
      const trimmed = text.trim();
      const looksLikeFile = /^([A-Za-z]:\\|\/|~\/)/.test(trimmed) && !trimmed.includes("\n") && trimmed.length < 500;
      if (looksLikeFile && ptyId) {
        e.preventDefault();
        invoke("write_pty_string", { ptyId, data: trimmed + "\r", raw: false }).catch(() => {});
        return;
      }
    }
  }, [ptyId]);

  if (!agentType) return null;

  return (
    <div className="absolute bottom-3 left-1/2 -translate-x-1/2 z-20">
      {visible ? (
        <div className="relative" style={{ width: "min(85vw, 520px)" }}>
          {/* Close button — circle anchored to top-right */}
          <button
            onClick={() => { if (input.trim()) setLastSent(input.trim()); setVisible(false); setInput(""); }}
            className="absolute -top-2.5 -right-2.5 w-5 h-5 rounded-full flex items-center justify-center shadow-md transition-colors z-10"
            style={{
              background: "var(--ct-surface)",
              border: "1px solid var(--ct-border)",
              color: "var(--ct-muted)",
            }}
            title="Close (Esc)"
          >
            <X size={8} strokeWidth={2} />
          </button>

          <div
            className="rounded-xl border shadow-xl backdrop-blur-sm overflow-hidden"
            style={{
              background: "color-mix(in srgb, var(--ct-surface) 95%, transparent)",
              borderColor: "var(--ct-border)",
            }}
          >
            <textarea
              ref={textareaRef}
              value={input}
              onChange={handleInput}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              onBlur={() => { if (!input) { if (lastSent) setVisible(false); } }}
              placeholder="Message agent... (Enter to send, Esc to close)"
              rows={1}
              className="w-full bg-transparent text-xs outline-none resize-none leading-relaxed px-3 py-2"
              style={{
                color: "var(--ct-fg)",
                minHeight: "32px",
                maxHeight: "200px",
              }}
            />
          </div>
        </div>
      ) : (
        <button
          onClick={() => {
            setVisible(true);
            if (lastSent && !input) {
              setInput(lastSent);
              requestAnimationFrame(() => {
                if (textareaRef.current) {
                  textareaRef.current.style.height = "0";
                  textareaRef.current.style.height = Math.min(textareaRef.current.scrollHeight, 200) + "px";
                }
              });
            }
          }}
          className="flex items-center gap-2 rounded-xl shadow-lg transition-all hover:brightness-125 px-3 py-1.5 backdrop-blur-md"
          style={{
            background: "color-mix(in srgb, var(--ct-surface) 70%, transparent)",
            border: "1px solid color-mix(in srgb, var(--ct-border) 60%, transparent)",
            color: "var(--ct-fg)",
            width: "min(85vw, 520px)",
          }}
          title="Chat with agent"
        >
          <MessageSquare size={12} strokeWidth={2} className="shrink-0 opacity-60" />
          <span className="text-[11px] truncate opacity-40">
            {lastSent || "Message agent..."}
          </span>
        </button>
      )}
    </div>
  );
}

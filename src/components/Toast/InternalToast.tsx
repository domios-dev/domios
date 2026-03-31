import { useState, useEffect, useCallback } from "react";

interface Toast {
  id: string;
  message: string;
  type: "prompt" | "complete" | "info";
  autoDismiss?: number; // ms, default 5000
}

interface InternalToastProps {
  pendingPrompt: { question: string } | null;
  paneId: string;
  agentType: string | null;
}

export function InternalToast({ pendingPrompt, paneId, agentType }: InternalToastProps) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());

  // Show toast when pending_prompt appears
  useEffect(() => {
    if (pendingPrompt && agentType) {
      const id = `prompt-${paneId}`;
      if (!dismissed.has(id)) {
        setToasts(prev => {
          if (prev.some(t => t.id === id)) return prev;
          return [...prev, { id, message: pendingPrompt.question, type: "prompt", autoDismiss: 8000 }];
        });
      }
    } else {
      // Clear prompt toast when prompt is resolved
      setToasts(prev => prev.filter(t => t.id !== `prompt-${paneId}`));
      setDismissed(prev => { const n = new Set(prev); n.delete(`prompt-${paneId}`); return n; });
    }
  }, [pendingPrompt, paneId, agentType, dismissed]);

  // Auto-dismiss
  useEffect(() => {
    const timers = toasts
      .filter(t => t.autoDismiss)
      .map(t => setTimeout(() => dismiss(t.id), t.autoDismiss));
    return () => timers.forEach(clearTimeout);
  }, [toasts]);

  const dismiss = useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id));
    setDismissed(prev => new Set([...prev, id]));
  }, []);

  if (toasts.length === 0) return null;

  return (
    <div className="absolute top-2 left-1/2 -translate-x-1/2 z-20 flex flex-col gap-1.5 pointer-events-none">
      {toasts.map(toast => (
        <div
          key={toast.id}
          className="pointer-events-auto rounded-lg px-3 py-1.5 border shadow-lg flex items-center gap-2 animate-in fade-in slide-in-from-top-2"
          style={{
            background: toast.type === "prompt"
              ? "color-mix(in srgb, var(--ct-yellow) 10%, var(--ct-surface))"
              : toast.type === "complete"
              ? "color-mix(in srgb, var(--ct-green) 10%, var(--ct-surface))"
              : "var(--ct-surface)",
            borderColor: toast.type === "prompt"
              ? "color-mix(in srgb, var(--ct-yellow) 30%, transparent)"
              : toast.type === "complete"
              ? "color-mix(in srgb, var(--ct-green) 30%, transparent)"
              : "var(--ct-border)",
            maxWidth: "400px",
          }}
        >
          <span className="text-[10px]" style={{
            color: toast.type === "prompt" ? "var(--ct-yellow)"
              : toast.type === "complete" ? "var(--ct-green)"
              : "var(--ct-fg)",
          }}>
            {toast.type === "prompt" ? "⚠" : toast.type === "complete" ? "✓" : "ℹ"}
          </span>
          <span className="text-[11px] truncate" style={{ color: "var(--ct-fg)" }}>
            {toast.message}
          </span>
          <button
            onClick={() => dismiss(toast.id)}
            className="shrink-0 ml-1 text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
          >
            <svg width="8" height="8" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <path d="M2 2l8 8M10 2l-8 8"/>
            </svg>
          </button>
        </div>
      ))}
    </div>
  );
}

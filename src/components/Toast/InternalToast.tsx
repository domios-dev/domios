import { useState, useEffect, useCallback } from "react";
import { Check, Info, TriangleAlert, X } from "lucide-react";

interface Toast {
  id: string;
  message: string;
  type: "prompt" | "complete" | "info";
  autoDismiss?: number;
}

interface InternalToastProps {
  pendingPrompt: { question: string } | null;
  tabId: string;
  agentType: string | null;
}

export function InternalToast({ pendingPrompt, tabId, agentType }: InternalToastProps) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (pendingPrompt && agentType) {
      const id = `prompt-${tabId}`;
      if (!dismissed.has(id)) {
        setToasts(prev => {
          if (prev.some(t => t.id === id)) return prev;
          return [...prev, { id, message: pendingPrompt.question, type: "prompt", autoDismiss: 8000 }];
        });
      }
    } else {
      setToasts(prev => prev.filter(t => t.id !== `prompt-${tabId}`));
      setDismissed(prev => { const n = new Set(prev); n.delete(`prompt-${tabId}`); return n; });
    }
  }, [pendingPrompt, tabId, agentType, dismissed]);

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
    <div className="absolute top-1 left-1/2 -translate-x-1/2 z-20 flex max-w-[min(520px,calc(100%-24px))] flex-col gap-1 pointer-events-none">
      {toasts.map(toast => (
        <div
          key={toast.id}
          className="pointer-events-auto flex h-7 max-w-full items-center gap-2 overflow-hidden rounded border px-2.5 text-[11px] leading-none shadow-[0_4px_14px_rgba(0,0,0,0.14)] ct-float animate-in fade-in slide-in-from-top-1"
          style={{
            background: toast.type === "prompt"
              ? "color-mix(in srgb, var(--ct-yellow) 8%, var(--ct-surface))"
              : toast.type === "complete"
              ? "color-mix(in srgb, var(--ct-green) 8%, var(--ct-surface))"
              : "color-mix(in srgb, var(--ct-surface) 92%, transparent)",
            borderColor: toast.type === "prompt"
              ? "color-mix(in srgb, var(--ct-yellow) 30%, transparent)"
              : toast.type === "complete"
              ? "color-mix(in srgb, var(--ct-green) 30%, transparent)"
              : "var(--ct-border)",
          }}
        >
          <span className="shrink-0" style={{
            color: toast.type === "prompt" ? "var(--ct-yellow)"
              : toast.type === "complete" ? "var(--ct-green)"
              : "var(--ct-fg)",
          }}>
            {toast.type === "prompt" ? <TriangleAlert size={12} /> : toast.type === "complete" ? <Check size={12} /> : <Info size={12} />}
          </span>
          <span className="min-w-0 truncate" style={{ color: "var(--ct-fg)" }}>
            {toast.message}
          </span>
          <button
            onClick={() => dismiss(toast.id)}
            className="ml-1 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--ct-muted)] transition-colors hover:bg-[var(--ct-hover-bg)] hover:text-[var(--ct-fg)]"
            aria-label="Dismiss"
          >
            <X size={11} strokeWidth={1.8} />
          </button>
        </div>
      ))}
    </div>
  );
}

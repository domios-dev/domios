import { AlertTriangle, X } from "lucide-react";
import type { PendingPrompt } from "../../lib/types";

// Renders a pane's interactive prompt (model picker, clarifying question,
// option menu) as a selectable card in the chat. Purely presentational: the
// owner decides what `onRespond`/`onCancel` do (see PaneSessionChat). Renders
// nothing without a prompt, so normal chat is untouched.
export function PendingPromptCard({
  pendingPrompt,
  onRespond,
  onCancel,
}: {
  pendingPrompt: PendingPrompt | null;
  onRespond: (value: string) => void;
  onCancel: () => void;
}) {
  if (!pendingPrompt) return null;
  return (
    <div className="px-3 pb-2 shrink-0" data-testid="pane-pending-prompt">
      <div
        className="rounded-lg px-4 py-3 border"
        style={{
          background: "color-mix(in srgb, var(--ct-yellow) 5%, var(--ct-surface))",
          borderColor: "color-mix(in srgb, var(--ct-yellow) 30%, transparent)",
        }}
      >
        <div className="flex items-center gap-2 mb-2">
          <AlertTriangle size={14} strokeWidth={2} style={{ stroke: "var(--ct-yellow)" }} />
          <span
            className="text-xs font-medium flex-1 min-w-0"
            style={{ color: "var(--ct-yellow)" }}
            data-testid="pane-pending-prompt-question"
          >
            {pendingPrompt.question}
          </span>
          <button
            type="button"
            data-testid="pane-pending-prompt-cancel"
            onMouseDown={(e) => e.preventDefault()}
            onClick={onCancel}
            className="inline-flex h-6 w-6 items-center justify-center rounded hover:brightness-125"
            style={{ background: "var(--ct-surface)", color: "var(--ct-muted-fg)" }}
            title="Cancel (Esc)"
          >
            <X size={12} />
          </button>
        </div>
        <div className="flex flex-wrap gap-2">
          {pendingPrompt.options.map((opt) => (
            <button
              key={opt.value}
              type="button"
              data-testid="pane-pending-prompt-option"
              data-prompt-value={opt.value}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onRespond(opt.value)}
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
  );
}

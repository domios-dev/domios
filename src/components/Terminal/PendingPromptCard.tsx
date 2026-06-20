import { useEffect, useState } from "react";
import { AlertTriangle, X } from "lucide-react";
import type { PendingPrompt } from "../../lib/types";

const WIZARD_SUBMIT_VALUE = "9000";
const WIZARD_MULTI_MASK_BASE = 10000;

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
  const isWizard = !!pendingPrompt.sections?.length;
  if (isWizard) {
    return (
      <WizardPendingPromptCard
        pendingPrompt={pendingPrompt}
        onRespond={onRespond}
        onCancel={onCancel}
      />
    );
  }
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

function WizardPendingPromptCard({
  pendingPrompt,
  onRespond,
  onCancel,
}: {
  pendingPrompt: PendingPrompt;
  onRespond: (value: string) => void;
  onCancel: () => void;
}) {
  const initialSelected = pendingPrompt.selected_values ?? [];
  const [selected, setSelected] = useState<Set<string>>(() => new Set(initialSelected));

  useEffect(() => {
    setSelected(new Set(pendingPrompt.selected_values ?? []));
  }, [pendingPrompt.question, pendingPrompt.selected_values]);

  const multiSelect = pendingPrompt.multi_select === true;
  const confirmMulti = () => {
    let mask = 0;
    for (const opt of pendingPrompt.options) {
      const idx = Number.parseInt(opt.value, 10) - 1;
      if (idx >= 0 && selected.has(opt.value)) mask += 1 << idx;
    }
    onRespond(String(WIZARD_MULTI_MASK_BASE + mask));
  };

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
          <div className="flex-1 min-w-0">
            <div className="flex flex-wrap items-center gap-1.5 mb-2" data-testid="pane-pending-prompt-sections">
              {pendingPrompt.sections?.map((section) => (
                <span
                  key={section.label}
                  className="px-2 py-0.5 rounded text-[11px] font-medium"
                  style={{
                    background: section.active
                      ? "color-mix(in srgb, var(--ct-yellow) 18%, transparent)"
                      : "var(--ct-surface)",
                    color: section.completed
                      ? "var(--ct-green)"
                      : section.active
                        ? "var(--ct-yellow)"
                        : "var(--ct-muted-fg)",
                    border: "1px solid color-mix(in srgb, var(--ct-border) 50%, transparent)",
                  }}
                >
                  {section.completed ? "✓ " : ""}{section.label}
                </span>
              ))}
            </div>
            <span
              className="text-xs font-medium block"
              style={{ color: "var(--ct-yellow)" }}
              data-testid="pane-pending-prompt-question"
            >
              {pendingPrompt.question}
            </span>
          </div>
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
          {pendingPrompt.options.map((opt) => {
            const checked = selected.has(opt.value);
            return (
              <button
                key={opt.value}
                type="button"
                data-testid="pane-pending-prompt-option"
                data-prompt-value={opt.value}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  if (!multiSelect) {
                    onRespond(opt.value);
                    return;
                  }
                  setSelected((prev) => {
                    const next = new Set(prev);
                    if (next.has(opt.value)) next.delete(opt.value);
                    else next.add(opt.value);
                    return next;
                  });
                }}
                className="px-3 py-1.5 rounded-md text-xs font-medium transition-colors hover:brightness-110"
                style={{
                  background: checked
                    ? "color-mix(in srgb, var(--ct-green) 20%, transparent)"
                    : "var(--ct-surface)",
                  color: checked ? "var(--ct-green)" : "var(--ct-fg)",
                  border: "1px solid color-mix(in srgb, var(--ct-border) 50%, transparent)",
                }}
              >
                {multiSelect ? `${checked ? "☑" : "☐"} ${opt.label}` : opt.label}
              </button>
            );
          })}
          {multiSelect && (
            <button
              type="button"
              data-testid="pane-pending-prompt-confirm"
              onMouseDown={(e) => e.preventDefault()}
              onClick={confirmMulti}
              className="px-3 py-1.5 rounded-md text-xs font-medium transition-colors hover:brightness-110"
              style={{
                background: "color-mix(in srgb, var(--ct-green) 20%, transparent)",
                color: "var(--ct-green)",
                border: "1px solid color-mix(in srgb, var(--ct-green) 35%, transparent)",
              }}
            >
              Confirm
            </button>
          )}
          {pendingPrompt.submit_available && (
            <button
              type="button"
              data-testid="pane-pending-prompt-submit"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onRespond(WIZARD_SUBMIT_VALUE)}
              className="px-3 py-1.5 rounded-md text-xs font-medium transition-colors hover:brightness-110"
              style={{
                background: "color-mix(in srgb, var(--ct-yellow) 20%, transparent)",
                color: "var(--ct-yellow)",
                border: "1px solid color-mix(in srgb, var(--ct-yellow) 35%, transparent)",
              }}
            >
              Submit
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

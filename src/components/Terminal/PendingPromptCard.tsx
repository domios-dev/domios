import { useEffect, useState } from "react";
import { Check, CornerDownLeft } from "lucide-react";
import type { PendingPrompt, PromptSection } from "../../lib/types";

const WIZARD_SUBMIT_VALUE = "9000";
const WIZARD_MULTI_MASK_BASE = 10000;

// Renders a pane's open terminal picker (model menu, clarifying question,
// multi-step wizard) as a first-class, inline message in the chat transcript.
// It is NOT dismissable: it mirrors the live terminal control surface and stays
// exactly as long as the backend reports `pane.pending_prompt`. The owner
// (PaneSessionChat) clears it by passing null once the picker closes. Purely
// presentational — `onRespond` carries the choice back through the existing
// mux_respond_prompt wiring.
export function PendingPromptCard({
  pendingPrompt,
  onRespond,
}: {
  pendingPrompt: PendingPrompt | null;
  onRespond: (value: string) => void;
}) {
  if (!pendingPrompt) return null;
  return <PromptCard pendingPrompt={pendingPrompt} onRespond={onRespond} />;
}

function PromptCard({
  pendingPrompt,
  onRespond,
}: {
  pendingPrompt: PendingPrompt;
  onRespond: (value: string) => void;
}) {
  const sections = pendingPrompt.sections ?? [];
  const isWizard = sections.length > 0;
  const multiSelect = pendingPrompt.multi_select === true;
  const submitAvailable = pendingPrompt.submit_available === true;

  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(pendingPrompt.selected_values ?? []),
  );
  useEffect(() => {
    setSelected(new Set(pendingPrompt.selected_values ?? []));
  }, [pendingPrompt.question, pendingPrompt.selected_values]);

  const handleOption = (value: string) => {
    if (!multiSelect) {
      onRespond(value);
      return;
    }
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(value)) next.delete(value);
      else next.add(value);
      return next;
    });
  };

  const confirmMulti = () => {
    let mask = 0;
    for (const opt of pendingPrompt.options) {
      const idx = Number.parseInt(opt.value, 10) - 1;
      if (idx >= 0 && selected.has(opt.value)) mask += 1 << idx;
    }
    onRespond(String(WIZARD_MULTI_MASK_BASE + mask));
  };

  const activeStep = Math.max(0, sections.findIndex((s) => s.active));
  const eyebrow = isWizard
    ? `Step ${activeStep + 1} of ${sections.length}`
    : "Your turn";

  return (
    <div className="mt-2" data-testid="pane-pending-prompt">
      <div
        className="rounded-xl px-4 py-3.5"
        style={{
          background: "color-mix(in srgb, var(--ct-accent) 7%, var(--ct-surface))",
          border: "1px solid color-mix(in srgb, var(--ct-accent) 26%, transparent)",
          borderLeft: "3px solid var(--ct-accent)",
        }}
      >
        <div className="mb-2 flex items-center gap-2">
          <LiveDot />
          <span
            className="text-[10px] font-semibold uppercase tracking-[0.12em]"
            style={{ color: "var(--ct-accent)" }}
          >
            {eyebrow}
          </span>
        </div>

        {isWizard && <Stepper sections={sections} />}

        <p
          className="text-sm font-medium leading-snug"
          style={{ color: "var(--ct-fg)" }}
          data-testid="pane-pending-prompt-question"
        >
          {pendingPrompt.question}
        </p>

        {multiSelect && (
          <p className="mt-1 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
            Select all that apply, then confirm.
          </p>
        )}

        <div className="mt-3 flex flex-col gap-1.5">
          {pendingPrompt.options.map((opt, idx) => {
            const checked = selected.has(opt.value);
            const keyLabel = /^\d+$/.test(opt.value) ? opt.value : String(idx + 1);
            return (
              <button
                key={opt.value}
                type="button"
                data-testid="pane-pending-prompt-option"
                data-prompt-value={opt.value}
                aria-pressed={multiSelect ? checked : undefined}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => handleOption(opt.value)}
                className="flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left text-sm transition-[filter,background-color] hover:brightness-125"
                style={{
                  background: checked
                    ? "color-mix(in srgb, var(--ct-accent) 16%, var(--ct-surface))"
                    : "var(--ct-surface)",
                  borderColor: checked
                    ? "color-mix(in srgb, var(--ct-accent) 55%, transparent)"
                    : "color-mix(in srgb, var(--ct-border) 60%, transparent)",
                  color: "var(--ct-fg)",
                }}
              >
                {multiSelect ? (
                  <CheckBox checked={checked} />
                ) : (
                  <KeyCap label={keyLabel} />
                )}
                <span className="min-w-0 flex-1 truncate">{opt.label}</span>
              </button>
            );
          })}
        </div>

        {(multiSelect || submitAvailable) && (
          <div className="mt-3 flex items-center justify-end gap-2">
            {multiSelect && (
              <button
                type="button"
                data-testid="pane-pending-prompt-confirm"
                onMouseDown={(e) => e.preventDefault()}
                onClick={confirmMulti}
                className="inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-semibold transition-[filter] hover:brightness-125"
                style={{
                  background: "color-mix(in srgb, var(--ct-green) 16%, transparent)",
                  color: "var(--ct-green)",
                  borderColor: "color-mix(in srgb, var(--ct-green) 40%, transparent)",
                }}
              >
                <Check size={13} strokeWidth={2.5} />
                Confirm
              </button>
            )}
            {submitAvailable && (
              <button
                type="button"
                data-testid="pane-pending-prompt-submit"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => onRespond(WIZARD_SUBMIT_VALUE)}
                className="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition-[filter] hover:brightness-110"
                style={{
                  background: "var(--ct-accent)",
                  color: "var(--ct-bg)",
                  border: "1px solid var(--ct-accent)",
                }}
              >
                <CornerDownLeft size={13} strokeWidth={2.5} />
                Submit
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function LiveDot() {
  return (
    <span className="relative inline-flex h-2 w-2 shrink-0" aria-hidden>
      <span
        className="absolute inline-flex h-full w-full rounded-full opacity-60 animate-ping motion-reduce:hidden"
        style={{ background: "var(--ct-accent)" }}
      />
      <span
        className="relative inline-flex h-2 w-2 rounded-full"
        style={{ background: "var(--ct-accent)" }}
      />
    </span>
  );
}

// Monospace keycap echoing the keystroke this option maps to in the terminal
// picker — the card's tie to the TUI it mirrors. aria-hidden: the label text
// carries the meaning; the cap is a visual cue.
function KeyCap({ label }: { label: string }) {
  return (
    <span
      aria-hidden
      className="inline-flex h-6 min-w-[1.5rem] shrink-0 items-center justify-center rounded-md px-1.5 font-mono text-[11px] font-semibold"
      style={{
        background: "color-mix(in srgb, var(--ct-accent) 12%, var(--ct-bg))",
        color: "var(--ct-accent)",
        border: "1px solid color-mix(in srgb, var(--ct-accent) 30%, transparent)",
        borderBottomWidth: 2,
      }}
    >
      {label}
    </span>
  );
}

function CheckBox({ checked }: { checked: boolean }) {
  return (
    <span
      aria-hidden
      className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded"
      style={{
        background: checked ? "var(--ct-accent)" : "transparent",
        border: `1px solid ${
          checked
            ? "var(--ct-accent)"
            : "color-mix(in srgb, var(--ct-border) 80%, transparent)"
        }`,
        color: "var(--ct-bg)",
      }}
    >
      {checked ? <Check size={12} strokeWidth={3} /> : null}
    </span>
  );
}

function Stepper({ sections }: { sections: PromptSection[] }) {
  return (
    <div
      className="mb-2.5 flex flex-wrap items-center gap-1.5"
      data-testid="pane-pending-prompt-sections"
    >
      {sections.map((section, i) => {
        const state = section.completed ? "done" : section.active ? "active" : "todo";
        const style =
          state === "done"
            ? {
                background: "color-mix(in srgb, var(--ct-green) 14%, transparent)",
                color: "var(--ct-green)",
                borderColor: "color-mix(in srgb, var(--ct-green) 35%, transparent)",
              }
            : state === "active"
              ? {
                  background: "color-mix(in srgb, var(--ct-accent) 16%, transparent)",
                  color: "var(--ct-accent)",
                  borderColor: "color-mix(in srgb, var(--ct-accent) 45%, transparent)",
                }
              : {
                  background: "var(--ct-surface)",
                  color: "var(--ct-muted-fg)",
                  borderColor: "color-mix(in srgb, var(--ct-border) 50%, transparent)",
                };
        return (
          <span
            key={section.label}
            className="inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-medium"
            style={style}
          >
            {state === "done" ? (
              <Check size={11} strokeWidth={3} />
            ) : (
              <span className="font-mono opacity-70">{i + 1}</span>
            )}
            {section.label}
          </span>
        );
      })}
    </div>
  );
}

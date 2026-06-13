import { useMemo, useState } from "react";
import { Check, CornerDownLeft } from "lucide-react";
import type { InteractiveQuestion as Question } from "@codeterm/chat-core";

const OTHER_KEY = "__other__";

export interface InteractiveAnswerHandler {
  (text: string, element: Question): void | Promise<void>;
}

export function InteractiveQuestion({
  question,
  onAnswer,
  answered,
}: {
  question: Question;
  onAnswer?: InteractiveAnswerHandler;
  /** Externally-driven answered state (kept across remounts via message id). */
  answered?: boolean;
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [otherText, setOtherText] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const isAnswered = answered || submitted;

  const toggle = (label: string) => {
    if (isAnswered) return;
    setSelected((prev) => {
      if (question.multiSelect) {
        const next = new Set(prev);
        next.has(label) ? next.delete(label) : next.add(label);
        return next;
      }
      return new Set([label]);
    });
  };

  const composedAnswer = useMemo(() => {
    const labels = question.options
      .map((o) => o.label)
      .filter((l) => selected.has(l));
    if (selected.has(OTHER_KEY) && otherText.trim()) {
      labels.push(otherText.trim());
    }
    return labels.join(", ");
  }, [question.options, selected, otherText]);

  const canSubmit = !isAnswered && composedAnswer.length > 0 && !!onAnswer;

  const submit = async () => {
    if (!canSubmit || !onAnswer) return;
    setSubmitted(true);
    try {
      await onAnswer(composedAnswer, question);
    } catch {
      setSubmitted(false);
    }
  };

  const otherSelected = selected.has(OTHER_KEY);

  return (
    <div
      data-testid="interactive-question"
      className="mb-3 w-full max-w-4xl min-w-0 mx-auto"
    >
      <div
        className="rounded-lg border px-4 py-3"
        style={{
          background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 5%, var(--ct-surface, #161b22))",
          borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
          opacity: isAnswered ? 0.6 : 1,
        }}
      >
        {question.header && (
          <div
            className="mb-1 text-[10px] font-medium uppercase tracking-wide"
            style={{ color: "var(--ct-accent, #58a6ff)" }}
          >
            {question.header}
          </div>
        )}
        <div className="mb-2.5 text-sm font-medium" style={{ color: "var(--ct-fg)" }}>
          {question.prompt}
        </div>

        <div className="flex flex-col gap-1.5">
          {question.options.map((opt) => {
            const checked = selected.has(opt.label);
            return (
              <button
                key={opt.label}
                type="button"
                disabled={isAnswered}
                onClick={() => toggle(opt.label)}
                className="flex items-start gap-2 rounded-md border px-2.5 py-1.5 text-left text-xs transition-colors hover:brightness-110 disabled:cursor-default"
                style={{
                  background: checked
                    ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)"
                    : "var(--ct-bg, #0d1117)",
                  borderColor: checked
                    ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 55%, transparent)"
                    : "color-mix(in srgb, var(--ct-border, #30363d) 60%, transparent)",
                  color: "var(--ct-fg)",
                }}
              >
                <span
                  className={`mt-0.5 flex h-3.5 w-3.5 shrink-0 items-center justify-center border ${question.multiSelect ? "rounded-sm" : "rounded-full"}`}
                  style={{
                    borderColor: checked ? "var(--ct-accent, #58a6ff)" : "var(--ct-muted-fg, #8b949e)",
                    background: checked ? "var(--ct-accent, #58a6ff)" : "transparent",
                  }}
                >
                  {checked && <Check size={10} strokeWidth={3} style={{ color: "var(--ct-bg, #0d1117)" }} />}
                </span>
                <span className="min-w-0">
                  <span className="block font-medium">{opt.label}</span>
                  {opt.description && (
                    <span className="block opacity-60">{opt.description}</span>
                  )}
                </span>
              </button>
            );
          })}

          {/* Free-text "Other" */}
          <div
            className="flex items-center gap-2 rounded-md border px-2.5 py-1.5"
            style={{
              background: otherSelected
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)"
                : "var(--ct-bg, #0d1117)",
              borderColor: otherSelected
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 55%, transparent)"
                : "color-mix(in srgb, var(--ct-border, #30363d) 60%, transparent)",
            }}
          >
            <button
              type="button"
              disabled={isAnswered}
              onClick={() => toggle(OTHER_KEY)}
              aria-label="Other"
              className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center border ${question.multiSelect ? "rounded-sm" : "rounded-full"} disabled:cursor-default`}
              style={{
                borderColor: otherSelected ? "var(--ct-accent, #58a6ff)" : "var(--ct-muted-fg, #8b949e)",
                background: otherSelected ? "var(--ct-accent, #58a6ff)" : "transparent",
              }}
            >
              {otherSelected && <Check size={10} strokeWidth={3} style={{ color: "var(--ct-bg, #0d1117)" }} />}
            </button>
            <input
              type="text"
              disabled={isAnswered}
              value={otherText}
              onChange={(e) => {
                setOtherText(e.target.value);
                if (e.target.value && !otherSelected) toggle(OTHER_KEY);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void submit();
                }
              }}
              placeholder="Other…"
              className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-[var(--ct-muted-fg)]"
              style={{ color: "var(--ct-fg)" }}
            />
          </div>
        </div>

        <div className="mt-2.5 flex items-center justify-end">
          {isAnswered ? (
            <span className="inline-flex items-center gap-1 text-[11px]" style={{ color: "var(--ct-green, #3fb950)" }}>
              <Check size={12} strokeWidth={3} /> Answered
            </span>
          ) : (
            <button
              type="button"
              disabled={!canSubmit}
              onClick={() => void submit()}
              className="inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors hover:brightness-110 disabled:opacity-40"
              style={{
                background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, transparent)",
                color: "var(--ct-accent, #58a6ff)",
                border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 45%, transparent)",
              }}
            >
              <CornerDownLeft size={12} strokeWidth={2.5} />
              Send answer
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

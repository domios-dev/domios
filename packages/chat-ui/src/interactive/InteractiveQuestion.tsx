import { useRef, useState } from "react";
import { Check, CornerDownLeft, MessageCircleQuestionMark } from "lucide-react";
import { emptyQuestionSelection, questionOptionValue, selectQuestionOption, selectQuestionFreeText,
  questionAnswerSelection, questionAnswerText, type InteractiveQuestion as Question } from "@codeterm/chat-core";


export interface InteractiveAnswerHandler {
  (text: string, element: Question, selection: { values: string[]; text?: string }): void | Promise<void>;
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
  const [selected, setSelected] = useState(emptyQuestionSelection);
  const [submitted, setSubmitted] = useState(false);
  const pending = useRef(false);
  const isAnswered = answered || submitted;
  const toggle = (value: string) => {
    if (isAnswered) return;
    setSelected((previous) => selectQuestionOption(question, previous, value));
  };
  const composedAnswer = questionAnswerText(question, selected);

  const canSubmit = !isAnswered && composedAnswer.length > 0 && !!onAnswer;

  const submit = async () => {
    if (!canSubmit || !onAnswer || pending.current) return;
    pending.current = true;
    setSubmitted(true);
    try {
      await onAnswer(composedAnswer, question, questionAnswerSelection(question, selected));
    } catch {
      setSubmitted(false);
    } finally { pending.current = false; }
  };

  const otherSelected = selected.freeText;

  return (
    <div data-testid="interactive-question" className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
      <div className={`ct-card${isAnswered ? " ct-card--quiet" : ""}`} style={{ opacity: isAnswered ? 0.72 : 1 }}>
        <div className="ct-card__head" style={{ alignItems: "flex-start" }}>
          <span className="ct-card__icon" aria-hidden="true"><MessageCircleQuestionMark size={14} /></span>
          <span className="min-w-0 flex-1 pt-0.5">
            {question.header && <span className="ct-card__meta block">{question.header}</span>}
            <span className="block text-[13px] font-medium leading-snug" style={{ color: "var(--ct-fg)" }}>{question.prompt}</span>
          </span>
        </div>

        <div className="ct-card__body">
          <div role="group" aria-label="Options" className="-mx-2 flex flex-col gap-px">
            {question.options.map((opt) => {
              const value = questionOptionValue(opt);
              const checked = selected.values.includes(value);
              return (
                <button
                  key={value}
                  type="button"
                  disabled={isAnswered}
                  aria-pressed={checked}
                  onClick={() => toggle(value)}
                  className={`ct-row flex items-start gap-2.5 px-2 py-1.5 text-left disabled:cursor-default${checked ? " ct-row--selected" : ""}`}
                  style={{ color: "var(--ct-fg)" }}
                >
                  <OptionMark checked={checked} multi={question.multiSelect} />
                  <span className="min-w-0">
                    <span className="block font-medium">{opt.label}</span>
                    {opt.description && <span className="block" style={{ color: "var(--ct-muted-fg)" }}>{opt.description}</span>}
                  </span>
                </button>
              );
            })}

            {question.allowFreeText !== false && <div className={`ct-row flex items-center gap-2.5 px-2 py-1.5${otherSelected ? " ct-row--selected" : ""}`}>
              <button
                type="button"
                disabled={isAnswered}
                onClick={() => setSelected((previous) => otherSelected ? { ...previous, freeText: false } : selectQuestionFreeText(question, previous, previous.text))}
                aria-label="Other"
                aria-pressed={otherSelected}
                className="flex shrink-0 disabled:cursor-default"
              >
                <OptionMark checked={otherSelected} multi={question.multiSelect} />
              </button>
              <input
                type="text"
                disabled={isAnswered}
                value={selected.text}
                onChange={(e) => {
                  setSelected((previous) => selectQuestionFreeText(question, previous, e.target.value));
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void submit();
                  }
                }}
                placeholder="Other…"
                className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-[var(--ct-muted-fg)]"
                style={{ color: "var(--ct-fg)" }}
              />
            </div>}
          </div>

          <div className="mt-2.5 flex items-center justify-end">
            {isAnswered ? (
              <span className="inline-flex items-center gap-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                <Check size={12} aria-hidden="true" style={{ color: "var(--ct-ok)" }} /> Answered
              </span>
            ) : (
              <button
                type="button"
                disabled={!canSubmit}
                onClick={() => void submit()}
                className="ct-bar-pill ct-bar-pill--primary disabled:opacity-50"
              >
                <CornerDownLeft size={12} aria-hidden="true" />
                Send answer
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function OptionMark({ checked, multi }: { checked: boolean; multi?: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={`mt-0.5 flex h-3.5 w-3.5 shrink-0 items-center justify-center border ${multi ? "rounded-[4px]" : "rounded-full"}`}
      style={{
        borderColor: checked ? "var(--ct-accent)" : "color-mix(in srgb, var(--ct-muted-fg) 60%, transparent)",
        background: checked ? "var(--ct-accent)" : "transparent",
      }}
    >
      {checked && <Check size={10} strokeWidth={3} style={{ color: "var(--ct-bg)" }} />}
    </span>
  );
}

import type { InteractiveQuestion } from "./types";

export interface QuestionSelection {
  values: readonly string[];
  freeText: boolean;
  text: string;
}

export const emptyQuestionSelection: QuestionSelection = { values: [], freeText: false, text: "" };
export const questionOptionValue = (option: { value?: string; label: string }): string => option.value ?? option.label;

export function selectQuestionOption(question: InteractiveQuestion, selected: QuestionSelection, value: string): QuestionSelection {
  if (!question.options.some((option) => questionOptionValue(option) === value)) return selected;
  if (!question.multiSelect) return { ...selected, values: [value], freeText: false };
  return { ...selected, values: selected.values.includes(value)
    ? selected.values.filter((item) => item !== value) : [...selected.values, value] };
}

export function selectQuestionFreeText(question: InteractiveQuestion, selected: QuestionSelection, text: string): QuestionSelection {
  if (question.allowFreeText === false) return selected;
  return { values: question.multiSelect ? selected.values : [], freeText: true, text };
}

export function questionAnswerSelection(question: InteractiveQuestion, selected: QuestionSelection) {
  const values = question.options.map(questionOptionValue).filter((value) => selected.values.includes(value));
  const text = question.allowFreeText !== false && selected.freeText ? selected.text.trim() : "";
  return { values, ...(text ? { text } : {}) };
}

export function questionAnswerText(question: InteractiveQuestion, selected: QuestionSelection): string {
  const answer = questionAnswerSelection(question, selected);
  return [...answer.values, ...(answer.text ? [answer.text] : [])].join(", ");
}

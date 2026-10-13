import type { InteractiveQuestion } from "@codeterm/chat-core";
import type { ChatSendStatus } from "../components/Terminal/chatSendStatus";

export interface RuntimeQuestionAnswer {
  questionId: string;
  values: string[];
  text?: string;
}

export interface AgentRuntimeQuestion {
  threadId: string;
  itemId: string;
  questions: Array<{ id: string; text: string; options: Array<{ value: string; label: string }>; multiSelect?: boolean; allowFreeText?: boolean }>;
  response: "message";
  createdAt: number;
  resolved?: { answers: RuntimeQuestionAnswer[]; sendId: string; status: ChatSendStatus } | null;
}

export function runtimeQuestionElement(item: AgentRuntimeQuestion, index: number): InteractiveQuestion {
  const question = item.questions[index];
  return {
    type: "question", id: `${item.itemId}:${question.id}`, toolUseId: item.itemId,
    prompt: question.text, options: question.options, multiSelect: question.multiSelect === true,
    allowFreeText: question.allowFreeText === true,
  };
}

export function collectRuntimeQuestionAnswer(
  item: AgentRuntimeQuestion, draft: RuntimeQuestionAnswer[], questionId: string, selection: { values: string[]; text?: string },
): RuntimeQuestionAnswer[] {
  const question = item.questions.find((candidate) => candidate.id === questionId);
  if (!question || item.resolved) return draft;
  const values = question.options.map((option) => option.value).filter((value) => selection.values.includes(value));
  if (!question.multiSelect && values.length > 1) return draft;
  const text = question.allowFreeText ? selection.text?.trim() : undefined;
  if (values.length === 0 && !text) return draft;
  return [...draft.filter((answer) => answer.questionId !== questionId), { questionId, values, ...(text ? { text } : {}) }];
}

export function runtimeQuestionProgress(item: AgentRuntimeQuestion, draft: RuntimeQuestionAnswer[]) {
  const index = item.questions.findIndex((question) => !draft.some((answer) => answer.questionId === question.id));
  return { index, complete: index < 0, answers: item.questions.flatMap((question) => draft.filter((answer) => answer.questionId === question.id)) };
}

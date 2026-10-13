import { expect, it } from "vitest";
import { collectRuntimeQuestionAnswer, runtimeQuestionElement, runtimeQuestionProgress, type AgentRuntimeQuestion } from "./agentRuntimeQuestions";

const item: AgentRuntimeQuestion = {
  threadId: "thread", itemId: "item", response: "message", createdAt: 1,
  questions: [
    { id: "first", text: "Fixture A", options: [{ value: "a", label: "Display A" }], allowFreeText: false },
    { id: "second", text: "Fixture B", options: [], allowFreeText: true },
  ],
};

it("projects the existing interactive card without a fake RPC prompt", () => {
  expect(runtimeQuestionElement(item, 0)).toEqual({ type: "question", id: "item:first", toolUseId: "item", prompt: "Fixture A",
    options: [{ value: "a", label: "Display A" }], multiSelect: false, allowFreeText: false });
});

it("collects a complete item once in provider question order", () => {
  const first = collectRuntimeQuestionAnswer(item, [], "first", { values: ["a"] });
  expect(runtimeQuestionProgress(item, first)).toMatchObject({ index: 1, complete: false });
  const complete = collectRuntimeQuestionAnswer(item, first, "second", { values: [], text: " custom " });
  expect(runtimeQuestionProgress(item, complete)).toEqual({ index: -1, complete: true,
    answers: [{ questionId: "first", values: ["a"] }, { questionId: "second", values: [], text: "custom" }] });
});

it("does not accept labels, unknown question IDs or disallowed free text as answers", () => {
  const draft: [] = [];
  expect(collectRuntimeQuestionAnswer(item, draft, "first", { values: ["Display A"] })).toBe(draft);
  expect(collectRuntimeQuestionAnswer(item, draft, "foreign", { values: ["a"] })).toBe(draft);
  expect(collectRuntimeQuestionAnswer(item, draft, "first", { values: [], text: "custom" })).toBe(draft);
});

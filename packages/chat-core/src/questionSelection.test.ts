import { expect, it } from "vitest";
import type { InteractiveQuestion } from "./types";
import { emptyQuestionSelection, questionAnswerSelection, questionAnswerText, selectQuestionFreeText, selectQuestionOption } from "./questionSelection";

const question: InteractiveQuestion = {
  type: "question", id: "q", toolUseId: "item", prompt: "fixture", multiSelect: false,
  options: [{ value: "native-a", label: "Display A" }, { value: "native-b", label: "Display A" }],
};

it("selects native values even when labels are identical", () => {
  const selected = selectQuestionOption(question, emptyQuestionSelection, "native-b");
  expect(questionAnswerSelection(question, selected)).toEqual({ values: ["native-b"] });
  expect(questionAnswerText(question, selected)).toBe("native-b");
  expect(selectQuestionOption(question, selected, "invalid")).toBe(selected);
});

it("uses the exact source string when a legacy row has no separate native value", () => {
  const legacy = { ...question, options: [{ label: "Source string" }] };
  expect(questionAnswerText(legacy, selectQuestionOption(legacy, emptyQuestionSelection, "Source string"))).toBe("Source string");
});

it("toggles multiselect values and serializes them in source order", () => {
  const multi = { ...question, multiSelect: true };
  const both = selectQuestionOption(multi, selectQuestionOption(multi, emptyQuestionSelection, "native-b"), "native-a");
  expect(questionAnswerSelection(multi, both)).toEqual({ values: ["native-a", "native-b"] });
  expect(questionAnswerSelection(multi, selectQuestionOption(multi, both, "native-b"))).toEqual({ values: ["native-a"] });
});

it("keeps free text separate from native values and respects the capability", () => {
  const selected = selectQuestionOption(question, emptyQuestionSelection, "native-a");
  expect(questionAnswerSelection(question, selectQuestionFreeText(question, selected, " custom "))).toEqual({ values: [], text: "custom" });
  const denied = { ...question, allowFreeText: false };
  expect(selectQuestionFreeText(denied, selected, "custom")).toBe(selected);
  const multi = { ...question, multiSelect: true };
  expect(questionAnswerSelection(multi, selectQuestionFreeText(multi, selected, "custom"))).toEqual({ values: ["native-a"], text: "custom" });
});

import { describe, expect, it } from "vitest";
import { answerablePrompt, promptMessagePart, isFreeTextPromptOption } from "./promptAnswers";

describe("answerablePrompt", () => {
  const prompt = { question: "opaque", options: ["a", "b"] };

  it("withholds answer cards unless the provider supports delivery", () => {
    for (const descriptor of [undefined, null, {}, { supports_prompt_answers: false }]) {
      expect(answerablePrompt(descriptor, prompt)).toBeNull();
    }
  });

  it("preserves supported prompts and absence", () => {
    const descriptor = { supports_prompt_answers: true };
    expect(answerablePrompt(descriptor, prompt)).toBe(prompt);
    expect(answerablePrompt(descriptor, null)).toBeNull();
  });
});

describe("promptMessagePart", () => {
  const prompt = { question: "opaque", options: [{ label: "a", value: "1" }] };
  const supported = { supports_prompt_answers: true };

  it("uses the capability to create an assistant prompt part", () => {
    expect(promptMessagePart({ supports_prompt_answers: false }, prompt)).toBeNull();
    expect(promptMessagePart(supported, prompt)?.prompt).toBe(prompt);
    expect(promptMessagePart(supported, null)).toBeNull();
  });

  it("suppresses answered terminal prompts and retains authoritative structured ones", () => {
    expect(promptMessagePart(supported, prompt, prompt.question)).toBeNull();
    expect(promptMessagePart({ ...supported, supports_structured_prompts: true }, prompt, prompt.question)?.prompt).toBe(prompt);
    expect(promptMessagePart(supported, prompt, "previous")?.prompt).toBe(prompt);
  });

  it("keeps part identity stable until the prompt changes", () => {
    expect(promptMessagePart(supported, prompt)?.id).toBe(promptMessagePart(supported, { ...prompt })?.id);
    expect(promptMessagePart(supported, prompt)?.id).not.toBe(promptMessagePart(supported, { ...prompt, question: "next" })?.id);
  });

  it("recognizes only free text choices", () => {
    expect(isFreeTextPromptOption({ label: "Other", value: "a" })).toBe(true);
    expect(isFreeTextPromptOption({ label: "Option one", value: "b" })).toBe(false);
  });
});

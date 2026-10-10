import { expect, it } from "vitest";
import { textPreviewSnippet } from "./localText";

it("keeps a short text whole and trims trailing space", () => {
  expect(textPreviewSnippet("\nline one\nline two\n\n")).toBe("line one\nline two");
});

it("clips long texts to a few lines and a few hundred characters", () => {
  const lines = Array.from({ length: 20 }, (_, index) => `line ${index}`).join("\n");
  expect(textPreviewSnippet(lines).split("\n")).toHaveLength(8);
  expect(textPreviewSnippet(lines).endsWith("…")).toBe(true);
  expect(textPreviewSnippet("x".repeat(1000)).length).toBe(421);
});

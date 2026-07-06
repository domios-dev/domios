import { describe, expect, it } from "vitest";

import { cleanMarkdown } from "./drawCard";

describe("cleanMarkdown", () => {
  it("strips inline code backticks", () => {
    expect(cleanMarkdown("run `codeterm plan`")).toContain("run codeterm plan");
  });
  it("strips bold markers", () => {
    expect(cleanMarkdown("this is **important**")).toBe("this is important");
  });
  it("strips strikethrough markers", () => {
    expect(cleanMarkdown("~~old~~ new")).toBe("old new");
  });
  it("strips heading marks", () => {
    expect(cleanMarkdown("## Overview")).toBe("Overview");
  });
  it("collapses links to their text", () => {
    expect(cleanMarkdown("see [the docs](https://x.io/y)")).toBe("see the docs");
  });
  it("collapses images to alt text", () => {
    expect(cleanMarkdown("![diagram](a.png)")).toBe("diagram");
  });
  it("removes blockquote marks", () => {
    expect(cleanMarkdown("> quoted line")).toBe("quoted line");
  });
  it("normalizes bullets to •", () => {
    expect(cleanMarkdown("- first\n* second\n+ third")).toBe("• first\n• second\n• third");
  });
  it("preserves underscores in identifiers", () => {
    expect(cleanMarkdown("task_body and section_ids")).toBe("task_body and section_ids");
  });
  it("leaves plain prose untouched", () => {
    expect(cleanMarkdown("just a sentence.")).toBe("just a sentence.");
  });
});

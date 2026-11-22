import { describe, expect, it } from "vitest";
import { looksLikeMermaidSource } from "./MermaidBlock";

describe("looksLikeMermaidSource", () => {
  it("does not classify a unified diff as Mermaid", () => {
    expect(looksLikeMermaidSource("--- file\n+++ file\n@@ -1,900 +1,13108 @@\n+flowchart TD")).toBe(false);
  });

  it("recognizes Mermaid after YAML frontmatter", () => {
    expect(looksLikeMermaidSource("---\ntitle: Example\n---\nflowchart TD\n  A --> B")).toBe(true);
  });

  it("recognizes a plain Mermaid keyword", () => {
    expect(looksLikeMermaidSource("  sequenceDiagram\n  Alice->>Bob: Hello")).toBe(true);
  });

  it("does not classify a YAML-only frontmatter block", () => {
    expect(looksLikeMermaidSource("---\ntitle: Example\n---\nother content")).toBe(false);
  });
});

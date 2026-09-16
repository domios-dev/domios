import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const stylesheet = readFileSync(resolve(process.cwd(), "src/styles/globals.css"), "utf8");
const sidebar = readFileSync(resolve(process.cwd(), "src/components/Sidebar/Sidebar.tsx"), "utf8");
const chat = readFileSync(resolve(process.cwd(), "src/components/Terminal/TabSessionChat.tsx"), "utf8");
const scrollbarRule = stylesheet.match(/\.ct-hover-reveal-scrollbar\s*\{([^}]*)\}/)?.[1] ?? "";
const thumbRule = stylesheet.match(/\.ct-hover-reveal-scrollbar::\-webkit-scrollbar-thumb\s*\{([^}]*)\}/)?.[1] ?? "";
const hoverThumbRule = stylesheet.match(/\.ct-hover-reveal-scrollbar:hover::\-webkit-scrollbar-thumb\s*\{([^}]*)\}/)?.[1] ?? "";
const scrollbarPseudoRule = stylesheet.match(/\.ct-hover-reveal-scrollbar::\-webkit-scrollbar\s*\{([^}]*)\}/)?.[1] ?? "";

describe("hover-revealed scrollbars", () => {
  it("hover_reveal_scrollbar shared class is applied to both scrolling surfaces", () => {
    expect(sidebar).toMatch(/className="ct-hover-reveal-scrollbar\b/);
    expect(chat).toMatch(/className="ct-hover-reveal-scrollbar\b/);
  });

  it("hover_reveal_scrollbar stylesheet keeps the Firefox fallback thin", () => {
    expect(scrollbarRule).not.toBe("");
    expect(scrollbarRule).toMatch(/scrollbar-width:\s*thin/);
    expect(scrollbarRule).toMatch(/scrollbar-color:/);
  });

  it("hover_reveal_scrollbar stylesheet makes the resting WebKit thumb transparent", () => {
    expect(thumbRule).not.toBe("");
    expect(thumbRule).toMatch(/background:\s*transparent/);
  });

  it("hover_reveal_scrollbar stylesheet gives the hovered WebKit thumb colour", () => {
    expect(hoverThumbRule).not.toBe("");
    expect(hoverThumbRule).toMatch(/background:\s*(?!transparent\b)[^;]+/);
  });

  it("hover_reveal_scrollbar stylesheet keeps scrollbar width fixed across states", () => {
    expect(scrollbarPseudoRule).not.toBe("");
    expect(scrollbarPseudoRule).toMatch(/width:\s*2px/);
    expect(scrollbarPseudoRule).toMatch(/height:\s*2px/);
    expect(hoverThumbRule).not.toMatch(/(?:width|height|display|overflow):/);
  });
});

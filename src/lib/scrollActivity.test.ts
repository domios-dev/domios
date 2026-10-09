import { afterEach, describe, expect, it, vi } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Window } from "happy-dom";
import { installScrollActivity } from "./scrollActivity";

const primitives = readFileSync(resolve(process.cwd(), "src/styles/primitives.css"), "utf8");

describe("scroll activity", () => {
  afterEach(() => vi.useRealTimers());

  it("marks the scrolling element until it rests", () => {
    vi.useFakeTimers();
    const doc = new Window().document as unknown as Document;
    const list = doc.createElement("div");
    doc.body.appendChild(list);
    const uninstall = installScrollActivity(doc);

    list.dispatchEvent(new (doc.defaultView as unknown as typeof globalThis).Event("scroll"));
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(600);
    list.dispatchEvent(new (doc.defaultView as unknown as typeof globalThis).Event("scroll"));
    vi.advanceTimersByTime(600);
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(400);
    expect(list.hasAttribute("data-scrolling")).toBe(false);
    uninstall();
  });

  it("shows the one shared thumb only while scrolling", () => {
    expect(primitives).toMatch(/::-webkit-scrollbar-thumb\s*\{[^}]*background-color:\s*transparent/);
    expect(primitives).toMatch(/\[data-scrolling\]::-webkit-scrollbar-thumb\s*\{[^}]*var\(--ct-scrollbar-thumb\)/);
    expect(primitives).toMatch(/--ct-scrollbar-thumb:\s*color-mix\(in srgb, var\(--ct-bg\) 80%, var\(--ct-fg\)\)/);
  });

  it("no view overrides the shared scrollbar", () => {
    const hits = execFileSync("git", ["grep", "-n", "-E", "scrollbar-width|scrollbar-color|scrollbarWidth|::-webkit-scrollbar", "--", "src", ":!src/styles/primitives.css", ":!*.test.*"], { encoding: "utf8" })
      .split("\n")
      .filter((line) => line && !/scrollbar-width:\s*none|\.ct-hscroll|xterm-viewport|\.crt-mode/.test(line));
    expect(hits).toEqual([]);
  });
});

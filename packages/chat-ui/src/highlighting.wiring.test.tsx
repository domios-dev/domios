import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatHighlightBudgetProvider, ChatHighlightRow } from "./ChatHighlightBudget";
import { DiffHunkLines, FileDiffCards } from "./toolcard/FileDiffCards";
import { RichMarkdown } from "./MarkdownBody";
import { CodeBlock } from "./CodeBlock";
import { knownLanguageFromPath } from "./codeLanguage";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Node", "Element"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : window[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) as unknown as MediaQueryList;
let observed: Element | null = null;
let intersect: IntersectionObserverCallback;
class Observer {
  constructor(callback: IntersectionObserverCallback) { intersect = callback; }
  observe(target: Element) { observed = target; }
  disconnect() {}
}
Object.defineProperty(globalThis, "IntersectionObserver", { value: Observer, configurable: true });

async function renderVisible(node: React.ReactElement) {
  observed = null;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<ChatHighlightBudgetProvider><ChatHighlightRow id="row">{node}</ChatHighlightRow></ChatHighlightBudgetProvider>));
  assert.ok(observed, "visibility observer targets mounted code");
  await act(async () => intersect([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver));
  return { host, dispose: async () => { await act(async () => root.unmount()); host.remove(); } };
}

test("visible .mjs hunks tokenize code and preserve diff markers and lines", async () => {
  const view = await renderVisible(<DiffHunkLines path="oracle.mjs" hunk={{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-const answer = 1;", "+const answer = 2;"] }} />);
  try {
    const lines = view.host.querySelectorAll('[data-testid="file-diff-line"]');
    assert.deepEqual(Array.from(lines, (line) => line.textContent), ["-const answer = 1;", "+const answer = 2;"]);
    for (const line of lines) assert.ok(line.querySelector("span.token"), "code tokens render for both sides");
    assert.deepEqual(Array.from(view.host.querySelectorAll('[data-testid="file-diff-marker"]'), (marker) => marker.textContent), ["-", "+"]);
  } finally { await view.dispose(); }
});

test("visible ts fence tokenizes through the markdown renderer", async () => {
  const view = await renderVisible(<RichMarkdown text={'```ts\nconst answer: number = 42;\n```'} />);
  try {
    assert.ok(Array.from(view.host.querySelectorAll("span.token")).some((token) => token.textContent === "const"));
    assert.ok(view.host.textContent?.includes("const answer: number = 42;"));
  } finally { await view.dispose(); }
});

test("file preview code uses .mjs language tokens", async () => {
  const view = await renderVisible(<CodeBlock text="const answer = 42;" language={knownLanguageFromPath("oracle.mjs")} showLineNumbers />);
  try { assert.ok(view.host.querySelector("span.token")); }
  finally { await view.dispose(); }
});

test("failed lazy grammar load preserves source and exposes plain text status", async () => {
  const { SyntaxHighlighter } = await import("./syntaxHighlighter");
  const { createGrammarLoader } = await import("./syntaxGrammarLoader");
  const failure = new Error("chunk unavailable");
  const logged: unknown[] = [];
  const loader = createGrammarLoader(async () => { throw failure; }, (error) => logged.push(error));
  const source = 'main = putStrLn "hello"';
  const view = await renderVisible(<SyntaxHighlighter language="haskell" loadGrammars={loader}>{source}</SyntaxHighlighter>);
  try {
    await act(async () => { assert.equal(await loader(), "failed"); });
    assert.equal(view.host.querySelector("code")?.textContent, source);
    assert.equal(view.host.querySelector("code span.token"), null);
    assert.ok(view.host.querySelector('[role="status"]'));
    assert.deepEqual(logged, [failure]);
  } finally { await view.dispose(); }
});

test("expanded edit tool card highlights .mjs hunks", async () => {
  const view = await renderVisible(<FileDiffCards initiallyExpanded rawCommand={false} event={{
    id: "edit", phase: "call", status: "success", kind: "edit", summary: "edit oracle.mjs",
    message: { id: "message", type: "tool_call", content: "" },
    fileDiffs: [{ path: "oracle.mjs", additions: 1, deletions: 1,
      hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-const before = 1;", "+const after = 2;"] }] }],
  }} />);
  try {
    assert.ok(view.host.querySelector('[role="region"]'));
    const lines = view.host.querySelectorAll('[data-testid="file-diff-line"]');
    assert.equal(lines.length, 2);
    for (const line of lines) assert.ok(line.querySelector("span.token"));
  } finally { await view.dispose(); }
});

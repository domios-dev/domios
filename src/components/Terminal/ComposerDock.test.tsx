import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ComposerDock } from "./ComposerDock";
import { ComposerCard } from "./ComposerCard";
import type { ComposerMode } from "./composerLayoutModel";

test("mode changes preserve the focused textarea and composition event boundary", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (mode: ComposerMode) => <ComposerDock mode={mode}><ComposerCard footer={<button>Model</button>}><textarea aria-label="Draft" defaultValue="opaque" /></ComposerCard><button>Send</button></ComposerDock>;
  try {
    await act(async () => root.render(render("floating")));
    const editor = host.querySelector("textarea")!;
    editor.focus();
    let commits = 0;
    editor.addEventListener("compositionend", () => commits++);
    editor.dispatchEvent(new Event("compositionstart", { bubbles: true }));
    await act(async () => root.render(render("docked")));
    assert.equal(host.querySelector("textarea"), editor);
    assert.equal(document.activeElement, editor);
    assert.equal(editor.value, "opaque");
    editor.dispatchEvent(new Event("compositionend", { bubbles: true }));
    assert.equal(commits, 1);
    await act(async () => root.render(render("floating")));
    assert.equal(host.querySelector("textarea"), editor);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

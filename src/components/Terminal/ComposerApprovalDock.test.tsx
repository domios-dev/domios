import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ComposerApprovalDock } from "./ComposerApprovalDock";

test("empty composer hotkey and click share one answer; snapshot clearing removes the dock", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const calls: string[] = [];
  const prompt = { question: "opaque-command", options: [{ label: "A", value: "once" }, { label: "B", value: "reject" }] };
  const respond = async (value: string) => { calls.push(value); };
  try {
    await act(async () => root.render(<><ComposerApprovalDock prompt={prompt} tabId="fixture" active onRespond={respond} /><textarea data-tab-id="fixture" /></>));
    const composer = host.querySelector("textarea")!;
    await act(async () => composer.dispatchEvent(new KeyboardEvent("keydown", { code: "Digit1", bubbles: true, cancelable: true })));
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    assert.deepEqual(calls, ["once"]);
    await act(async () => root.render(null));
    assert.equal(host.querySelector('[aria-label="Approval dock"]'), null);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("background approvals and nonempty composer drafts do not consume digits", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const calls: string[] = [];
  const prompt = { question: "opaque", options: [{ label: "A", value: "accept" }] };
  try {
    await act(async () => root.render(<><ComposerApprovalDock prompt={prompt} tabId="fixture" active={false} onRespond={async (value) => { calls.push(value); }} /><textarea data-tab-id="fixture" defaultValue="draft" /></>));
    await act(async () => host.querySelector("textarea")!.dispatchEvent(new KeyboardEvent("keydown", { code: "Digit1", bubbles: true })));
    assert.deepEqual(calls, []);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

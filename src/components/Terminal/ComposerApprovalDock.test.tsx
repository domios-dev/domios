import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest } from "../../lib/bridge";
import type { PendingPrompt } from "../../lib/types";
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

test("permission with structured file_diffs renders syntax-highlighted diff and Rule 2 buttons without raw method label", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const calls: string[] = [];
  const prompt: PendingPrompt = {
    question: "Writes src/app.ts (+1/-1 lines)\nEdit src/app.ts\nKind: edit\nAllow this tool call?",
    options: [
      { label: "Allow once", value: "allow_once" },
      { label: "Always allow", value: "allow_always" },
      { label: "Reject", value: "reject_once" },
    ],
    file_diffs: [{
      path: "src/app.ts",
      kind: "modify",
      additions: 1,
      deletions: 1,
      hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-const x = 1;", "+const x = 2;"] }],
    }],
  };
  try {
    await act(async () => root.render(<ComposerApprovalDock prompt={prompt} tabId="diff-tab" active onRespond={async (v) => { calls.push(v); }} />));
    assert.ok(!host.textContent?.includes("request_permission"));
    assert.equal(host.querySelector('[data-testid="approval-dock-title"]')?.textContent, "Edit src/app.ts");
    assert.equal(host.querySelector('[data-testid="approval-dock-diff-stats"]')?.textContent?.trim(), "+1 -1");
    assert.ok(host.querySelector('[data-testid="file-diff-hunks"]'));
    const buttons = [...host.querySelectorAll("button")].map((b) => b.textContent?.trim());
    assert.deepEqual(buttons, ["Allow once", "Always allow", "Reject"]);
    const rejectBtn = [...host.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Reject")!;
    await act(async () => rejectBtn.click());
    assert.deepEqual(calls, ["reject_once"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("form elicitation validates fields, ignores digit hotkeys, and submits JSON payload", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const calls: string[] = [];
  const prompt: PendingPrompt = {
    question: "Configure deploy",
    options: [],
    elicitation: {
      mode: "form",
      message: "Configure deploy",
      required: ["region"],
      fields: [
        { name: "region", type: "string", title: "Region", required: true },
        { name: "dryRun", type: "boolean", title: "Dry run", default: true },
      ],
    },
  };
  try {
    await act(async () => root.render(<ComposerApprovalDock prompt={prompt} tabId="form-tab" active onRespond={async (v) => { calls.push(v); }} />));
    const input = host.querySelector<HTMLInputElement>('input[type="text"]')!;
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { code: "Digit1", bubbles: true })));
    assert.deepEqual(calls, []);
    const submitBtn = host.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    await act(async () => submitBtn.click());
    assert.ok(host.querySelector('[role="alert"]')?.textContent?.includes("Region is required"));
    assert.deepEqual(calls, []);
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
      setter?.call(input, "eu-west-1");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => submitBtn.click());
    assert.deepEqual(calls, [JSON.stringify({ action: "accept", content: { region: "eu-west-1", dryRun: true } })]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("url elicitation displays host safely and opens via host open_url", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const calls: string[] = [];
  const opened: unknown[] = [];
  __setInvokeForTest(async (cmd, args) => {
    opened.push([cmd, args]);
    return undefined as never;
  });
  const prompt: PendingPrompt = {
    question: "Sign in to GitHub",
    options: [{ label: "Open sign-in", value: "accept" }, { label: "Decline", value: "decline" }],
    elicitation: {
      mode: "url",
      message: "Sign in to GitHub",
      url: "https://github.com/login/oauth/authorize?state=secret",
      host: "github.com",
    },
  };
  try {
    await act(async () => root.render(<ComposerApprovalDock prompt={prompt} tabId="url-tab" active onRespond={async (v) => { calls.push(v); }} />));
    assert.equal(host.querySelector('[data-testid="elicitation-url-host"]')?.textContent, "github.com");
    assert.ok(!host.textContent?.includes("state=secret"));
    const openBtn = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("Open sign-in"))!;
    await act(async () => openBtn.click());
    assert.deepEqual(opened, [["open_url", { url: "https://github.com/login/oauth/authorize?state=secret" }]]);
    assert.deepEqual(calls, ["accept"]);
  } finally {
    __setInvokeForTest(null);
    await act(async () => root.unmount());
    host.remove();
  }
});

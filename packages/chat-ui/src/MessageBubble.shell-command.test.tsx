// Run: npx tsx packages/chat-ui/src/MessageBubble.shell-command.test.tsx
import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";

import { MessageBubble } from "./MessageBubble";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

type TestCase = { name: string; run: () => void | Promise<void> };
const tests: TestCase[] = [];
function test(name: string, run: TestCase["run"]) {
  tests.push({ name, run });
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const windowInstance = new Window({ url: "http://localhost/shell-command-tests" });
const windowAny = windowInstance as unknown as Window & typeof globalThis;
(globalThis as any).window = windowAny;
(globalThis as any).document = windowAny.document;
Object.defineProperty(globalThis, "navigator", { configurable: true, value: windowAny.navigator });
(globalThis as any).HTMLElement = windowAny.HTMLElement;
(globalThis as any).HTMLButtonElement = windowAny.HTMLButtonElement;
(globalThis as any).Node = windowAny.Node;
(globalThis as any).MouseEvent = windowAny.MouseEvent;
(globalThis as any).requestAnimationFrame = (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0);
(globalThis as any).cancelAnimationFrame = (id: number) => clearTimeout(id);
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

const clipboardWrites: string[] = [];
Object.defineProperty(windowAny.navigator, "clipboard", {
  configurable: true,
  value: { writeText: async (text: string) => { clipboardWrites.push(text); } },
});

async function waitForEffects() {
  await act(async () => {
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await Promise.resolve();
  });
}

async function render(
  message: NormalizedChatMessage,
  props: Partial<{ onRerunShellCommand: (c: string) => void; onJumpToShellCommand: (m: NormalizedChatMessage) => void }> = {},
): Promise<{ host: HTMLElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<MessageBubble message={message} {...props} />);
  });
  await waitForEffects();
  return { host, root };
}

function byTestId(host: HTMLElement, id: string): HTMLElement | null {
  return host.querySelector(`[data-testid="${id}"]`);
}

function click(el: Element) {
  el.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }));
}

const base = (over: Partial<NormalizedChatMessage>): NormalizedChatMessage => ({
  id: "s1",
  type: "shell_command",
  content: over.shellCommand ?? "",
  ...over,
});

test("renders a running command with the spinner glyph", async () => {
  const { host, root } = await render(base({ shellCommand: "npm test", shellStatus: "running", shellHasOutputStart: true }));
  const root_ = byTestId(host, "chat-shell-command");
  assert(root_, "shell command item rendered");
  assert(root_!.getAttribute("data-shell-status") === "running", "status attr is running");
  assert((byTestId(host, "chat-shell-command-text")?.textContent ?? "").includes("npm test"), "command text shown");
  assert(byTestId(host, "chat-shell-status-glyph"), "status glyph rendered");
  assert(!byTestId(host, "chat-shell-exit"), "running command shows no exit badge");
  await act(async () => { root.unmount(); });
});

test("renders a failed command with exit code and cwd", async () => {
  const { host, root } = await render(base({ shellCommand: "boom", shellStatus: "failed", shellExitCode: 127, shellCwd: "/home/me/repo" }));
  assert(byTestId(host, "chat-shell-command")?.getAttribute("data-shell-status") === "failed", "failed status");
  const exit = byTestId(host, "chat-shell-exit");
  assert(exit && exit.textContent!.includes("127"), "exit code surfaced");
  const cwd = byTestId(host, "chat-shell-cwd");
  assert(cwd && cwd.textContent!.length > 0, "cwd decoration shown");
  await act(async () => { root.unmount(); });
});

test("copy affordance writes the command to the clipboard", async () => {
  clipboardWrites.length = 0;
  const { host, root } = await render(base({ shellCommand: "ls -la", shellStatus: "ok" }));
  const copyBtn = byTestId(host, "chat-shell-copy");
  assert(copyBtn, "copy button present");
  await act(async () => { click(copyBtn!); });
  await waitForEffects();
  assert(clipboardWrites.includes("ls -la"), "command copied to clipboard");
  await act(async () => { root.unmount(); });
});

test("rerun affordance fires the callback with the command", async () => {
  const reruns: string[] = [];
  const { host, root } = await render(
    base({ shellCommand: "make build", shellStatus: "ok" }),
    { onRerunShellCommand: (c) => reruns.push(c) },
  );
  const rerunBtn = byTestId(host, "chat-shell-rerun");
  assert(rerunBtn, "rerun button present when handler supplied");
  await act(async () => { click(rerunBtn!); });
  assert(reruns.length === 1 && reruns[0] === "make build", "rerun callback received command");
  await act(async () => { root.unmount(); });
});

test("jump affordance fires the callback with the message", async () => {
  const jumps: NormalizedChatMessage[] = [];
  const msg = base({ shellCommand: "grep foo", shellStatus: "ok" });
  const { host, root } = await render(msg, { onJumpToShellCommand: (m) => jumps.push(m) });
  const jumpBtn = byTestId(host, "chat-shell-jump");
  assert(jumpBtn, "jump button present when handler supplied");
  await act(async () => { click(jumpBtn!); });
  assert(jumps.length === 1 && jumps[0].id === msg.id, "jump callback received the message");
  await act(async () => { root.unmount(); });
});

test("rerun/jump buttons hidden without handlers", async () => {
  const { host, root } = await render(base({ shellCommand: "ls", shellStatus: "ok" }));
  assert(!byTestId(host, "chat-shell-rerun"), "no rerun without handler");
  assert(!byTestId(host, "chat-shell-jump"), "no jump without handler");
  assert(byTestId(host, "chat-shell-copy"), "copy is always available");
  await act(async () => { root.unmount(); });
});

test("A/D-only finished command still renders boundary + exit + cwd", async () => {
  const { host, root } = await render(base({ shellCommand: "git status", shellStatus: "ok", shellExitCode: 0, shellCwd: "/x", shellHasOutputStart: false }));
  assert(byTestId(host, "chat-shell-command"), "item renders without output-start");
  assert(byTestId(host, "chat-shell-cwd"), "cwd shown in A/D-only mode");
  await act(async () => { root.unmount(); });
});

(async () => {
  let passed = 0;
  let failed = 0;
  for (const t of tests) {
    try {
      await t.run();
      passed++;
      console.log(`  ✓ ${t.name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${t.name}: ${(e as Error).message}`);
    }
  }
  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
})();

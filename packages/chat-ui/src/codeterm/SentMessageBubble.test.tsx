import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { recognize } from "./recognize";
import { SentMessageBubble } from "./SentMessageBubble";
import { CodetermCard } from "./CodetermCard";

const windowInstance = new Window({ url: "http://localhost/sent-message-test" });
const windowAny = windowInstance as unknown as Window & typeof globalThis;
(globalThis as any).window = windowAny;
(globalThis as any).document = windowAny.document;
(globalThis as any).HTMLElement = windowAny.HTMLElement;
(globalThis as any).Node = windowAny.Node;
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

let passed = 0;
let failed = 0;
function assert(cond: unknown, name: string) {
  if (cond) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}`);
  }
}

async function render(node: React.ReactElement): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(node);
  });
  return host;
}

function inv(cmd: string) {
  const i = recognize(cmd);
  if (!i) throw new Error(`recognize failed: ${cmd}`);
  return i;
}

const PLAN = "pl_" + "a".repeat(32);
const TASK = "task_" + "b".repeat(32);

// basic bubble + pane-name resolution
await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --pane 915fae8e "## Update\n- done"`)}
      resolvePaneName={(r) => (r === "915fae8e" ? "Puccini" : undefined)}
    />,
  );
  const text = host.textContent ?? "";
  assert(host.querySelector('[data-testid="chat-sent-message"]') != null, "renders sent-message bubble");
  assert(text.includes("Puccini"), "renders resolved recipient name");
  assert(text.includes("Update") && text.includes("done"), "renders markdown body content");
})();

// pane-name fallback when unresolvable
await (async () => {
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --pane 915fae8e "hi"`)} />);
  assert((host.textContent ?? "").includes("pane 915fae8e"), "falls back to 'pane <ref>'");
})();

// long body collapses past 8 lines, expands on click
await (async () => {
  const body = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --pane bda "${body}"`)} />);
  const expand = host.querySelector('[data-testid="chat-sent-expand"]') as HTMLButtonElement | null;
  assert(expand != null, "shows expand affordance past threshold");
  assert(!(host.textContent ?? "").includes("line 15"), "collapsed body hides later lines");
  await act(async () => {
    expand!.click();
  });
  assert((host.textContent ?? "").includes("line 15"), "expanded body reveals all lines");
})();

// failed send shows failure glyph
await (async () => {
  const host = await render(
    <SentMessageBubble inv={inv(`codeterm send --pane bda "x"`)} result="error: pane not found" />,
  );
  assert((host.textContent ?? "").includes("✗"), "failed send shows failure glyph");
})();

// mesh send shows peer
await (async () => {
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --pane bda --mesh mini "hi"`)} />);
  assert((host.textContent ?? "").includes("mini"), "mesh send renders peer");
})();

// task-id mention in send body becomes a link, click fires onOpenPlan
await (async () => {
  let opened: [string, string | undefined] | null = null;
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --pane bda "see ${TASK} in ${PLAN}"`)}
      onOpenPlan={(p, t) => {
        opened = [p, t];
      }}
    />,
  );
  const link = host.querySelector('[data-testid="chat-task-link"]') as HTMLButtonElement | null;
  assert(link != null, "task id in body renders link");
  await act(async () => {
    link!.click();
  });
  assert(opened != null && opened![0] === PLAN && opened![1] === TASK, "clicking task link opens plan+task");
})();

// plan card click-to-open intent
await (async () => {
  let opened: [string, string | undefined] | null = null;
  const host = await render(
    <CodetermCard
      inv={inv(`codeterm plan task update --plan ${PLAN} --task ${TASK} --status done`)}
      expanded={false}
      onToggle={() => {}}
      onOpenPlan={(p, t) => {
        opened = [p, t];
      }}
    />,
  );
  const open = host.querySelector('[data-testid="chat-open-plan"]') as HTMLButtonElement | null;
  assert(open != null, "plan card shows 'open in plan' affordance");
  await act(async () => {
    open!.click();
  });
  assert(opened != null && opened![0] === PLAN && opened![1] === TASK, "clicking plan card opens plan+task");
})();

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);

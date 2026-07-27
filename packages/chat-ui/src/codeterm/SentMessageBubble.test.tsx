import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { recognize } from "./recognize";
import { SentMessageBubble } from "./SentMessageBubble";
import { CodetermCard } from "./CodetermCard";
import { crossPaneBubbleClassName, CROSS_PANE_SHELL_TESTID } from "../crossPaneBubble";
import { MessageBubble } from "../MessageBubble";

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
const SECTION = "sec_vecy77";

// basic bubble + pane-name resolution
await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab 915fae8e "## Update\n- done"`)}
      resolvePaneName={(r) => (r === "915fae8e" ? "Puccini" : undefined)}
    />,
  );
  const text = host.textContent ?? "";
  const shell = host.querySelector(`[data-testid="${CROSS_PANE_SHELL_TESTID}"]`) as HTMLElement | null;
  const bubble = host.querySelector('[data-testid="chat-sent-message"]') as HTMLElement | null;
  assert(shell != null, "renders left-aligned cross-pane shell");
  assert(shell!.className.includes("justify-start"), "outbound cross-pane is left aligned");
  assert(bubble != null, "renders sent-message body");
  assert(host.querySelector('[data-testid="chat-sent-header"]') != null, "renders sent-message header");
  assert(bubble!.className === crossPaneBubbleClassName("text-base"), "outbound uses shared cross-pane bubble class");
  assert(!bubble!.className.includes("border"), "shared bubble has no accent border frame");
  assert(text.includes("Puccini"), "renders resolved recipient name");
  assert(text.includes("Update") && text.includes("done"), "renders markdown body content");
})();

for (const [command, expectedBody] of [
  [`codeterm send --tab recipient "inline message"`, "inline message"],
  [`codeterm send --tab recipient --from-file update.md`, "update.md"],
  [`codeterm send --tab recipient --stdin`, ""],
  [`codeterm send --tab recipient`, ""],
] as const) {
  const host = await render(
    <SentMessageBubble
      inv={inv(command)}
      result="OK (sent)"
      resolvePaneName={(ref) => (ref === "recipient" ? "Recipient" : undefined)}
    />,
  );
  const text = host.textContent ?? "";
  const bubble = host.querySelector('[data-testid="chat-sent-message"]');
  assert(bubble != null && bubble.textContent!.trim().length > 0, `${command} renders a non-blank outgoing row`);
  assert(text.includes("Recipient") && text.includes("✓"), `${command} retains recipient and delivered status`);
  if (expectedBody) assert(text.includes(expectedBody), `${command} retains its body descriptor`);
}

// section-id mention in normal chat renders a deep-link card with the shared selector contract.
await (async () => {
  let opened: [string, string | undefined, string | undefined] | null = null;
  const host = await render(
    <MessageBubble
      message={{
        id: "assistant-plan-ref",
        provider: "codex",
        type: "assistant",
        content: `See ${SECTION} in ${PLAN}`,
      }}
      onOpenPlan={(p, focus, kind) => {
        opened = [p, focus, kind];
      }}
      resolvePlanRef={(planId, focusId, focusKind) => ({
        title: focusKind === "section" ? "Cross workspace section" : "Owner workspace plan",
        planTitle: "Owner workspace plan",
        sectionTitle: focusKind === "section" ? "Cross workspace section" : undefined,
        state: "resolved",
        rawLabel: focusId ?? planId,
        fullRef: focusId ? `${planId}/${focusId}` : planId,
      })}
    />,
  );
  const card = host.querySelector('[data-testid="chat-section-link"][data-ref-kind="section"]') as HTMLButtonElement | null;
  assert(card != null, "assistant section ref renders shared link selector");
  assert(card?.textContent?.includes("Cross workspace section"), "assistant ref card uses resolved title");
  await act(async () => {
    card!.click();
  });
  assert(
    opened != null && opened![0] === PLAN && opened![1] === SECTION && opened![2] === "section",
    "clicking section ref opens plan+section",
  );
})();

// Outbound messages stay compact while inbound messages use an activity card.
await (async () => {
  const outboundHost = await render(
    <SentMessageBubble inv={inv(`codeterm send --tab bda "hello outbound"`)} />,
  );
  const inboundHost = await render(
    <MessageBubble
      message={{
        id: "inbound-1",
        provider: "lmstudio",
        type: "user",
        content: "hello inbound",
        fromLabel: "Watcher",
        fromPane: "abc12345",
        fromModel: "fable",
        inboundKind: "tab",
      }}
    />,
  );
  const outboundBubble = outboundHost.querySelector('[data-testid="chat-sent-message"]') as HTMLElement;
  const inboundCard = inboundHost.querySelector('[data-testid="chat-inbound-message"]') as HTMLElement | null;
  assert(outboundBubble != null, "outbound message renders its compact bubble");
  assert(inboundCard != null, "inbound message renders its activity card");
  assert(
    inboundHost.querySelector('[data-testid="chat-cross-pane-bubble"]') == null,
    "inbound activity does not regress to the legacy cross-pane bubble",
  );
})();

// inbound current-pane marker resolves placeholder labels from pane id
await (async () => {
  const host = await render(
    <MessageBubble
      message={{
        id: "inbound-current-pane",
        provider: "codex",
        type: "user",
        content: "hello self",
      }}
      origin={{ kind: "pane", title: "CURRENT Pane", paneIdShort: "915fae8e", model: "codex" }}
      resolvePaneName={(r) => (r === "915fae8e" ? "Puccini" : undefined)}
    />,
  );
  const header = host.querySelector('[data-testid="chat-inbound-message"]') as HTMLElement | null;
  const text = header?.textContent ?? "";
  assert(text.includes("Puccini"), "current-pane marker renders resolved recipient name");
  assert(!text.includes("CURRENT Pane"), "current-pane marker placeholder is replaced");
})();

// inbound current-pane structured metadata falls back to pane id when label is blank
await (async () => {
  const host = await render(
    <MessageBubble
      message={{
        id: "inbound-current-pane-blank",
        provider: "codex",
        type: "user",
        content: "hello self",
        fromLabel: "",
        fromPane: "915fae8e",
        inboundKind: "tab",
      }}
    />,
  );
  assert((host.textContent ?? "").includes("915fae8e"), "blank current-pane label falls back to short Tab id");
})();

// pane-name fallback when unresolvable
await (async () => {
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --tab 915fae8e "hi"`)} />);
  assert((host.textContent ?? "").includes("tab 915fae8e"), "falls back to 'tab <ref>'");
})();

// long body collapses past 8 lines, expands on click
await (async () => {
  const body = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --tab bda "${body}"`)} />);
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
    <SentMessageBubble inv={inv(`codeterm send --tab bda "x"`)} result="error: Tab not found" />,
  );
  assert((host.textContent ?? "").includes("✗"), "failed send shows failure glyph");
})();

// mesh send shows peer
await (async () => {
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --tab bda --mesh mini "hi"`)} />);
  assert((host.textContent ?? "").includes("mini"), "mesh send renders peer");
})();

// task-id mention in send body becomes a link, click fires onOpenPlan
await (async () => {
  let opened: [string, string | undefined] | null = null;
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab bda "see ${TASK} in ${PLAN}"`)}
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

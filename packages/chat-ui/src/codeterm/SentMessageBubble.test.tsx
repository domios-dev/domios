import { referenceMarkerText } from "./referenceMarkers";
import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { recognize } from "./recognize";
import { SentMessageBubble } from "./SentMessageBubble";
import { CodetermCard } from "./CodetermCard";
import { MessageBubble } from "../MessageBubble";
import type { ToolEvent } from "@codeterm/chat-core";
import { CodetermActionCard } from "../toolcard/CodetermActionCard";

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

// basic bubble + tab-name resolution
await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab 915fae8e "## Update\n- done"`)}
      resolveTabName={(r) => (r === "915fae8e" ? "Puccini" : undefined)}
    />,
  );
  const text = host.textContent ?? "";
  const shell = host.querySelector('[data-testid="chat-sent-to-card"]') as HTMLElement | null;
  const bubble = host.querySelector('[data-testid="chat-sent-message"]') as HTMLElement | null;
  assert(shell != null, "renders left-aligned cross-tab shell");
  assert(bubble != null, "renders sent-message body");
  assert(host.querySelector('[data-testid="chat-sent-header"]') != null, "renders sent-message header");
  assert(text.includes("Puccini"), "renders resolved recipient name");
  assert(text.includes("Tab ID 915fae8e"), "resolved recipient id remains visible as secondary metadata");
  assert(text.includes("Update") && text.includes("done"), "renders markdown body content");
  assert(host.querySelector('[data-testid="raw-command-disclosure"]') == null, "outbound message has no raw command disclosure");
})();

await (async () => {
  const host = await render(<SentMessageBubble inv={inv("codeterm send --tab recipient --from-file update.md")} result="OK (sent)" />);
  assert(host.querySelector('[data-testid="raw-command-disclosure"]') == null, "unresolved staged message has no raw command disclosure");
  assert(host.textContent?.includes("update.md"), "unresolved staged message renders its source descriptor");
})();

await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv('codeterm send --tab recipient "hello"')}
      result={'OK (pending)\nCODETERM_RESOLVED_TEXT="hello"'}
    />,
  );
  const status = host.querySelector('[data-testid="chat-sent-status"]');
  assert(status?.getAttribute("aria-label") === "pending", "unconfirmed write renders pending status");
  assert(status?.getAttribute("aria-label") === "pending", "pending delivery does not show a delivered or failed mark");
  assert(host.textContent?.includes("hello"), "written payload echo stays visible while delivery is pending");
})();

for (const [command, expectedBody] of [
  [`codeterm send --tab recipient "inline message"`, "inline message"],
  [`codeterm send --tab recipient --from-file update.md`, "update.md"],
  [`codeterm send --tab recipient --file update.md`, "update.md"],
  [`codeterm send --tab recipient --stdin`, ""],
  [`codeterm send --tab recipient`, ""],
] as const) {
  const host = await render(
    <SentMessageBubble
      inv={inv(command)}
      result="OK (sent)"
      resolveTabName={(ref) => (ref === "recipient" ? "Recipient" : undefined)}
    />,
  );
  const text = host.textContent ?? "";
  const bubble = host.querySelector('[data-testid="chat-sent-message"]');
  assert(bubble != null && bubble.textContent!.trim().length > 0, `${command} renders a non-blank outgoing row`);
  assert(text.includes("Recipient") && host.querySelector('[data-testid="chat-sent-status"]')?.getAttribute("aria-label") === "sent", `${command} retains recipient and sent status`);
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
        content: `See ${referenceMarkerText("section", SECTION, PLAN)}`,
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
        fromTab: "abc12345",
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
    inboundHost.querySelector('[data-testid="chat-cross-tab-bubble"]') == null,
    "inbound activity does not regress to the legacy cross-tab bubble",
  );
})();

// inbound current-tab marker resolves placeholder labels from tab id
await (async () => {
  const host = await render(
    <MessageBubble
      message={{
        id: "inbound-current-tab",
        provider: "codex",
        type: "user",
        content: "hello self",
      }}
      origin={{ kind: "tab", title: "CURRENT Tab", tabIdShort: "915fae8e", model: "codex" }}
      resolveTabName={(r) => (r === "915fae8e" ? "Puccini" : undefined)}
    />,
  );
  const header = host.querySelector('[data-testid="chat-inbound-message"]') as HTMLElement | null;
  const text = header?.textContent ?? "";
  assert(text.includes("Puccini"), "current-tab marker renders resolved recipient name");
  assert(!text.includes("CURRENT Tab"), "current-tab marker placeholder is replaced");
})();

// inbound current-tab structured metadata falls back to tab id when label is blank
await (async () => {
  const host = await render(
    <MessageBubble
      message={{
        id: "inbound-current-tab-blank",
        provider: "codex",
        type: "user",
        content: "hello self",
        fromLabel: "",
        fromTab: "915fae8e",
        inboundKind: "tab",
      }}
    />,
  );
  assert((host.textContent ?? "").includes("915fae8e"), "blank current-tab label falls back to short Tab id");
})();

// tab-name fallback when unresolvable
await (async () => {
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --tab 915fae8e "hi"`)} />);
  assert((host.textContent ?? "").includes("tab 915fae8e"), "falls back to 'tab <ref>'");
})();

// long body collapses past 8 lines, expands on click
await (async () => {
  const body = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
  const host = await render(<SentMessageBubble inv={inv(`codeterm send --tab bda "${body}"`)} />);
  const expand = host.querySelector('[data-testid="chat-sent-expand"]') as HTMLButtonElement | null;
  assert(expand == null, "card owns expansion instead of a second line counter");
  assert((host.textContent ?? "").includes("line 15"), "the card bounds the complete body without truncating source lines");
})();

// failed send shows failure glyph
await (async () => {
  const host = await render(
    <SentMessageBubble inv={inv(`codeterm send --tab bda "x"`)} result="error: Tab not found" />,
  );
  assert(host.querySelector('[data-testid="chat-sent-status"]')?.getAttribute("aria-label") === "failed", "failed send shows failure state");
})();

for (const [delivery, label] of [["submit_unconfirmed", "submit_unconfirmed"], ["interrupted", "interrupted"], ["unresolved", "unresolved"]] as const) {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab bda "x"`)}
      result={JSON.stringify({ result: { delivery } })}
    />,
  );
  assert(host.querySelector('[data-testid="chat-sent-status"]')?.getAttribute("aria-label") === label, `${delivery} label remains distinct`);
}

for (const status of ["submit_unconfirmed", "interrupted", "unresolved"] as const) {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab bda "x"`)}
      activityStatus={status}
    />,
  );
  assert(host.querySelector('[data-testid="chat-sent-status"]')?.getAttribute("aria-label") === status, `${status} activity status remains distinct`);
}

await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab bda "x"`)}
      result="Exit code: 2\nOK (sent)"
      activityStatus="error"
    />,
  );
  assert(host.querySelector('[data-testid="chat-sent-status"]')?.getAttribute("aria-label") === "failed", "activity error overrides a positive result label");
  assert(host.querySelector('[role="alert"]') != null, "contradictory send output retains visible error evidence");
})();

await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab bda "x"`)}
      result="Tab not found"
    />,
  );
  assert(host.querySelector('[role="alert"]')?.textContent === "Tab not found", "plain-text send failures retain their visible reason");
})();

await (async () => {
  const host = await render(
    <SentMessageBubble
      inv={inv(`codeterm send --tab bda "x"`)}
      activityStatus="success"
    />,
  );
  assert(host.querySelector('[data-testid="chat-sent-status"]')?.getAttribute("aria-label") === "unresolved", "a successful tool turn without delivery evidence stays unresolved");
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
      inv={inv(`codeterm send --tab bda 'see ${referenceMarkerText("task", TASK, PLAN)}'`)}
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

await (async () => {
  const body = "## Deployment\n" + "detail line\n".repeat(12);
  const message = {
    id: "narrow-send", type: "tool_call" as const, content: "", toolName: "exec", toolKind: "codeterm" as const,
    toolInput: { command: `pwsh.exe -Command 'codeterm send --tab 915fae8e --from-pad message.md --json'` },
  };
  const event: ToolEvent = {
    id: "narrow-send", phase: "result", status: "success", kind: "codeterm",
    summary: message.toolInput.command, detail: message.toolInput.command,
    result: JSON.stringify({ result: { delivery: "sent", sent_message: { text: body, truncated: false } } }),
    message,
  };
  const host = await render(<CodetermActionCard event={event} expanded={false} onToggle={() => {}} resolveTabName={(ref) => ref === "915fae8e" ? "Courier · Launch" : undefined} />);
  (host.firstElementChild as HTMLElement).style.width = "320px";
  const card = host.querySelector('[data-testid="chat-sent-to-card"]');
  const status = host.querySelector('[role="status"][aria-label="sent"]');
  const expand = host.querySelector('[data-testid="chat-sent-expand"]');
  const technical = host.querySelector('[data-testid="raw-command-disclosure"] button');
  assert(card != null && status != null, "narrow send card exposes recipient card and truthful sent status");
  assert(host.textContent?.includes("Courier · Launch") && host.textContent.includes("Deployment"), "narrow send card shows resolved recipient, Team context and body preview");
  assert(expand == null && technical == null, "long body uses the shared card bounds and no inline technical details");
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

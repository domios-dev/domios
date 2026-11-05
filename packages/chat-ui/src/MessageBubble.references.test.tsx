import assert from "node:assert/strict";
import test from "node:test";
import { act } from "react";
import type { MessageSegment } from "@codeterm/chat-core";
import type { ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { MessageBubble, mount } from "./dsBlocks.testdata";

test("user reference markers survive structured envelope projection and receive resolved labels", async () => {
  const view = await mount();
  const marker = '<domios type="task" id="task_d5r5nn" plan="pl_94ornz" />';
  const before = 'B5_MARKER_15\nPlain task_d5r5nn and pl_94ornz stay prose.\nExplicit: ';
  const after = '\nCode: `task_d5r5nn`.';
  const content = before + marker + after;
  const segments: MessageSegment[] = [
    { type: "text", text: before },
    { type: "unknown_frame", raw: marker, attrs: { type: "task", id: "task_d5r5nn", plan: "pl_94ornz" }, body: "" },
    { type: "text", text: after },
  ];
  const opened: unknown[][] = [];
  const requested: unknown[][] = [];
  let resolved: ResolvedChatPlanRef = {
    title: "task_d5r5nn", planTitle: "pl_94ornz", state: "loading",
    rawLabel: "task_d5r5nn", fullRef: "pl_94ornz/task_d5r5nn",
  };
  const render = (parts?: MessageSegment[]) => view.render(<MessageBubble
    message={{ id: "marker-user", type: "user", content, segments: parts }}
    onOpenPlan={(...args) => opened.push(args)}
    resolvePlanRef={(...args) => { requested.push(args); return resolved; }}
  />);
  try {
    await render(segments);
    const chip = view.host.querySelector<HTMLButtonElement>('[data-testid="chat-task-link"]');
    assert.ok(chip, "structured USER message retains its explicit chip before resolution");
    assert.equal(chip.disabled, true);
    assert.match(chip.getAttribute("aria-label") ?? "", /task_d5r5nn/);
    resolved = { ...resolved, title: "CON-3 R1 shared fixture worker lifecycle", state: "resolved" };
    await render(segments);
    assert.equal(view.host.querySelector('[data-testid="chat-task-link"]'), chip);
    assert.equal(chip.disabled, false);
    assert.match(chip.getAttribute("aria-label") ?? "", /CON-3 R1 shared fixture worker lifecycle/);
    assert.equal(view.host.querySelectorAll('[data-testid="chat-task-link"]').length, 1);
    assert.match(view.host.textContent ?? "", /Plain task_d5r5nn and pl_94ornz stay prose\./);
    assert.match(view.host.textContent ?? "", /Code: `task_d5r5nn`\./);
    assert.ok(requested.length > 0);
    assert.ok(requested.every((args) => JSON.stringify(args) === JSON.stringify(["pl_94ornz", "task_d5r5nn", "task"])));
    await act(async () => chip.click());
    assert.deepEqual(opened, [["pl_94ornz", "task_d5r5nn", "task"]]);
    await render();
    assert.match(view.host.querySelector('[data-testid="chat-task-link"]')?.getAttribute("aria-label") ?? "", /CON-3 R1 shared fixture worker lifecycle/);
    await render([...segments, { type: "attachment", kind: "file", path: "/quote", label: "Reply", body: null, status: null, reference: "earlier", excerpt: "Quoted text" }]);
    assert.equal(view.host.querySelectorAll('[data-testid="chat-task-link"]').length, 1);
    assert.match(view.host.querySelector('[data-testid="chat-task-link"]')?.getAttribute("aria-label") ?? "", /CON-3 R1 shared fixture worker lifecycle/);
    assert.match(view.host.textContent ?? "", /Quoted text/);
  } finally { await view.close(); }
});

test("envelope reference projection preserves code examples and other unknown frame bodies", async () => {
  const view = await mount();
  const marker = '<domios type="task" id="task_d5r5nn" plan="pl_94ornz" />';
  const segments: MessageSegment[] = [
    { type: "text", text: "Code: `" },
    { type: "unknown_frame", raw: marker, attrs: { type: "task" }, body: "" },
    { type: "text", text: "`. " },
    { type: "unknown_frame", raw: '<domios type="other" />', attrs: { type: "other" }, body: "Preserved body" },
    { type: "partial_frame", raw: '<domios type="task"', attrs: {}, body: "Partial body" },
  ];
  try {
    await view.render(<MessageBubble message={{ id: "code-user", type: "user", content: "", segments }} onOpenPlan={() => assert.fail("code must stay inert")} />);
    assert.equal(view.host.querySelectorAll('[data-testid="chat-task-link"]').length, 0);
    assert.equal(view.host.textContent, `Code: \`${marker}\`. Preserved bodyPartial body`);
  } finally { await view.close(); }
});

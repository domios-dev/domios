import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Tab } from "../../../lib/types";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { PERF_SPEC_ID, PERF_SPEC_SECTIONS } from "../perfFixtureSections";
import type { Plan } from "../types";
import { PlanPlanView, PlanSpecView } from "./PlanDocumentView";

const sendTrackedChatMessage = vi.hoisted(() => vi.fn());
const transcript = vi.hoisted(() => ({ messages: [] as Array<Record<string, unknown>> }));

vi.mock("../../Terminal/chatSendStatus", () => ({ sendTrackedChatMessage }));
vi.mock("../../Terminal/useChatSession", () => ({
  useChatSession: () => ({ allMessages: transcript.messages, loading: false }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PLAN_BODY = [
  "Ship the **login** flow first.",
  "",
  "The rollout spans two lines",
  "and keeps sessions warm.",
  "",
  "- Rotate the signing key",
  "- Keep `refresh` tokens",
].join("\n");

const ASK_PLAN: Plan = {
  ...FIXTURE_PLAN,
  id: "pl_ask",
  primary_spec_id: PERF_SPEC_ID,
  spec_links: [],
  goal: null,
  sections: [{ id: "sec_ask", title: "Rollout", body: PLAN_BODY, subsections: [], canvas: FIXTURE_PLAN.sections[0].canvas }],
  owner: { tab_id: "orch-1", session_id: "sess-1", title: "Orchestrator" },
};

function ownerTab(state: "idle" | "working" = "idle"): Record<string, Tab> {
  return {
    "orch-1": {
      id: "orch-1",
      title: "Hudson",
      agent_type: "fixture-agent",
      agent_session_id: "sess-1",
      agent_state: { state },
    } as unknown as Tab,
  };
}

let mounted: Array<{ root: Root; host: HTMLElement }> = [];

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  await act(async () => {});
  mounted.push({ root, host });
  return {
    host,
    rerender: async (next: ReactElement) => {
      await act(async () => root.render(next));
    },
  };
}

function setReducedMotion(reduced: boolean) {
  window.matchMedia = ((query: string) => ({
    matches: reduced && query.includes("reduce"),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

function rect(top: number) {
  return { top, bottom: top + 20, left: 0, right: 300, width: 300, height: 20, x: 0, y: top, toJSON: () => ({}) };
}

function stubTop(element: Element, top: number) {
  Object.defineProperty(element, "getBoundingClientRect", { configurable: true, value: () => rect(top) });
}

function q<T extends Element = HTMLElement>(root: ParentNode, testId: string): T | null {
  return root.querySelector<T>(`[data-testid="${testId}"]`);
}

function textNode(root: Node, needle: string): Text {
  const walk = (node: Node): Text | null => {
    if (node.nodeType === 3 && (node as Text).data.includes(needle)) return node as Text;
    for (const child of Array.from(node.childNodes)) {
      const found = walk(child);
      if (found) return found;
    }
    return null;
  };
  const found = walk(root);
  if (!found) throw new Error(`text node containing ${JSON.stringify(needle)} not found`);
  return found;
}

async function select(start: Text, startOffset: number, end: Text, endOffset: number, top = 240) {
  Object.defineProperty(window.Range.prototype, "getBoundingClientRect", { configurable: true, value: () => rect(top) });
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  await act(async () => {
    document.dispatchEvent(new Event("selectionchange"));
  });
}

async function clearSelection() {
  document.getSelection()?.removeAllRanges();
  await act(async () => {
    document.dispatchEvent(new Event("selectionchange"));
  });
}

async function hover(element: Element) {
  await act(async () => {
    element.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  });
}

async function key(element: Element, code: string, init: KeyboardEventInit = {}) {
  await act(async () => {
    element.dispatchEvent(new KeyboardEvent("keydown", { code, key: code, bubbles: true, cancelable: true, ...init }));
  });
}

async function click(element: Element | null) {
  expect(element).not.toBeNull();
  await act(async () => {
    (element as HTMLElement).click();
  });
  await act(async () => {});
}

async function typeQuestion(host: HTMLElement, value: string) {
  const input = q<HTMLTextAreaElement>(host, "plan-ask-input");
  expect(input).not.toBeNull();
  const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, value);
    input!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function sentReference(call = 0): Record<string, unknown> {
  const text = sendTrackedChatMessage.mock.calls[call][2] as string;
  return JSON.parse(text.slice(text.lastIndexOf("\n") + 1)) as Record<string, unknown>;
}

function planBody(host: HTMLElement): HTMLElement {
  return host.querySelector<HTMLElement>('[data-testid="plan-plan-section"][data-section-id="sec_ask"] [data-plan-ask-body]')!;
}

function specBody(host: HTMLElement, sectionId: string): HTMLElement {
  return host.querySelector<HTMLElement>(`[data-testid="plan-spec-section"][data-section-id="${sectionId}"] [data-plan-ask-body]`)!;
}

function specSource(sectionId: string): string {
  return PERF_SPEC_SECTIONS.find((section) => section.id === sectionId)!.body;
}

beforeEach(() => {
  setReducedMotion(false);
  transcript.messages = [];
  sendTrackedChatMessage.mockReset();
  sendTrackedChatMessage.mockResolvedValue({ state: "accepted" });
});

afterEach(async () => {
  for (const { root, host } of mounted) {
    await act(async () => root.unmount());
    host.remove();
  }
  mounted = [];
  document.getSelection()?.removeAllRanges();
});

describe("Plan reader paragraph Ask", () => {
  it("reveals one labelled gutter trigger that glides between hovered paragraphs and list items", async () => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const body = planBody(host);
    const paragraph = body.querySelector("p")!;
    const item = body.querySelectorAll("li")[0]!;
    stubTop(paragraph, 110);
    stubTop(item, 300);

    expect(q(host, "plan-ask-trigger")).toBeNull();
    await hover(paragraph);
    const trigger = q<HTMLButtonElement>(host, "plan-ask-trigger")!;
    expect(trigger.getAttribute("aria-label")).toBe("Ask about this paragraph");
    expect(trigger.dataset.askMode).toBe("paragraph");
    expect(trigger.dataset.motion).toBe("glide");
    expect(trigger.style.transform).toBe("translateY(110px)");
    expect(body.contains(trigger)).toBe(false);

    await hover(item);
    expect(q(host, "plan-ask-trigger")).toBe(trigger);
    expect(trigger.style.transform).toBe("translateY(300px)");
  });

  it("snaps instead of gliding under reduced motion", async () => {
    setReducedMotion(true);
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    await hover(planBody(host).querySelector("p")!);
    expect(q<HTMLElement>(host, "plan-ask-trigger")!.dataset.motion).toBe("snap");
    await key(planBody(host).querySelector("p")!, "Enter");
    expect(q<HTMLElement>(host, "plan-ask-popover")!.dataset.motion).toBe("snap");
  });

  it("is keyboard reachable from a focused paragraph and returns focus to it", async () => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const body = planBody(host);
    const [first, second] = Array.from(body.querySelectorAll<HTMLElement>("p"));
    expect(first.tabIndex).toBe(0);
    expect(first.getAttribute("aria-keyshortcuts")).toBe("Enter");

    await act(async () => first.focus());
    const trigger = q<HTMLButtonElement>(host, "plan-ask-trigger")!;
    expect(trigger).not.toBeNull();
    await key(first, "Tab");
    expect(document.activeElement).toBe(trigger);
    await key(trigger, "Tab");
    expect(document.activeElement).toBe(second);
    await act(async () => first.focus());
    await key(first, "Tab");
    await key(trigger, "Tab", { shiftKey: true });
    expect(document.activeElement).toBe(first);

    await key(first, "Enter");
    const popover = q(host, "plan-ask-popover")!;
    expect(popover.getAttribute("role")).toBe("dialog");
    expect(document.activeElement).toBe(q(host, "plan-ask-input"));
    expect(popover.querySelector("label")?.textContent).toBe("Ask about this paragraph");
    await key(q(host, "plan-ask-input")!, "Escape");
    expect(q(host, "plan-ask-popover")).toBeNull();
    expect(document.activeElement).toBe(first);
  });

  it("sends a paragraph question with the paragraph quote and its exact block offsets", async () => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const paragraph = planBody(host).querySelector("p")!;
    await act(async () => paragraph.focus());
    await key(paragraph, "Enter");
    expect(q(host, "plan-ask-quote")!.textContent).toBe("Ship the login flow first.");
    await typeQuestion(host, "Why login first?");
    await click(q(host, "plan-ask-send"));

    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);
    const [tabId, askId, text] = sendTrackedChatMessage.mock.calls[0] as [string, string, string];
    expect(tabId).toBe("orch-1");
    expect(text).toContain("Why login first?");
    expect(sentReference()).toEqual({
      ask_id: askId,
      plan_id: "pl_ask",
      document: "plan",
      spec_id: null,
      section_id: "sec_ask",
      section_title: "Rollout",
      mode: "paragraph",
      source_start: 0,
      source_end: "Ship the **login** flow first.".length,
      rendered_start: 0,
      rendered_end: "Ship the login flow first.".length,
      offset_unit: "utf16_code_unit",
      quote: "Ship the login flow first.",
    });
  });

  it("gives an exact selection precedence over paragraph hover, then returns to paragraph mode", async () => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const body = planBody(host);
    const paragraph = body.querySelector("p")!;
    stubTop(paragraph, 110);
    await hover(paragraph);

    const tail = textNode(body, " flow first.");
    await select(tail, 1, tail, 11, 240);
    const trigger = q<HTMLElement>(host, "plan-ask-trigger")!;
    expect(trigger.dataset.askMode).toBe("selection");
    expect(trigger.getAttribute("aria-label")).toBe("Ask about selected text");
    expect(trigger.style.transform).toBe("translateY(240px)");

    await click(trigger);
    expect(q(host, "plan-ask-quote")!.textContent).toBe("flow first");
    expect(q(host, "plan-ask-popover")!.querySelector("label")?.textContent).toBe("Ask about selected text");
    await typeQuestion(host, "Is it blocking?");
    await click(q(host, "plan-ask-send"));
    expect(sentReference()).toMatchObject({
      mode: "selection",
      quote: "flow first",
      source_start: PLAN_BODY.indexOf("flow first"),
      source_end: PLAN_BODY.indexOf("flow first") + "flow first".length,
    });

    await click(q(host, "plan-ask-close"));
    await clearSelection();
    await hover(paragraph);
    expect(q<HTMLElement>(host, "plan-ask-trigger")!.dataset.askMode).toBe("paragraph");
  });

  it.each([
    {
      name: "a phrase crossing inline emphasis",
      pick: (body: HTMLElement) => [textNode(body, "Ship the "), 5, textNode(body, " flow first."), 5] as const,
      quote: "the login flow",
      source: "the **login** flow",
    },
    {
      name: "a multi-line phrase",
      pick: (body: HTMLElement) => {
        const node = textNode(body, "two lines");
        return [node, node.data.indexOf("two"), node, node.data.indexOf("keeps") + 5] as const;
      },
      quote: "two lines\nand keeps",
      source: "two lines\nand keeps",
    },
    {
      name: "a list item",
      pick: (body: HTMLElement) => {
        const node = textNode(body, "Rotate the signing key");
        return [node, 0, node, node.data.length] as const;
      },
      quote: "Rotate the signing key",
      source: "Rotate the signing key",
    },
    {
      name: "inline code inside a list item",
      pick: (body: HTMLElement) => {
        const node = textNode(body, "refresh");
        return [node, 0, node, node.data.length] as const;
      },
      quote: "refresh",
      source: "refresh",
    },
  ])("maps $name to its exact quote and source offsets", async ({ pick, quote, source }) => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const [start, startOffset, end, endOffset] = pick(planBody(host));
    await select(start, startOffset, end, endOffset);
    await click(q(host, "plan-ask-trigger"));
    expect(q(host, "plan-ask-quote")!.textContent).toBe(quote);
    await typeQuestion(host, "Explain this.");
    await click(q(host, "plan-ask-send"));

    const sourceStart = PLAN_BODY.indexOf(source);
    const reference = sentReference();
    expect(reference).toMatchObject({ plan_id: "pl_ask", section_id: "sec_ask", mode: "selection", quote });
    expect(reference.source_start).toBe(sourceStart);
    expect(reference.source_end).toBe(sourceStart + source.length);
    expect(PLAN_BODY.slice(reference.source_start as number, reference.source_end as number)).toBe(source);
  });

  it("cancel and Escape close without sending, restore focus, and leave the document untouched", async () => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const body = planBody(host);
    const before = body.textContent;
    const item = body.querySelector<HTMLElement>("li")!;

    await act(async () => item.focus());
    await key(item, "Enter");
    await typeQuestion(host, "Draft that never leaves");
    await click(q(host, "plan-ask-cancel"));
    expect(q(host, "plan-ask-popover")).toBeNull();
    expect(document.activeElement).toBe(item);

    await key(item, "Enter");
    await typeQuestion(host, "Another draft");
    await key(q(host, "plan-ask-popover")!, "Escape");
    expect(q(host, "plan-ask-popover")).toBeNull();
    expect(document.activeElement).toBe(item);

    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
    expect(body.textContent).toBe(before);
    expect(ASK_PLAN.sections[0].body).toBe(PLAN_BODY);
  });

  it("reports a missing or stopped owner honestly and never sends", async () => {
    const unowned: Plan = { ...ASK_PLAN, owner: { tab_id: null, session_id: null, title: null } };
    const missing = await render(<PlanPlanView mode="focus" planId={unowned.id} plans={[unowned]} tabs={ownerTab()} />);
    const missingParagraph = planBody(missing.host).querySelector("p")!;
    await act(async () => missingParagraph.focus());
    await key(missingParagraph, "Enter");
    expect(q(missing.host, "plan-ask-owner-missing")).not.toBeNull();
    expect(q(missing.host, "plan-ask-input")).toBeNull();
    expect(q(missing.host, "plan-ask-send")).toBeNull();

    const stopped = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={{}} />);
    const stoppedParagraph = planBody(stopped.host).querySelector("p")!;
    await act(async () => stoppedParagraph.focus());
    await key(stoppedParagraph, "Enter");
    expect(q(stopped.host, "plan-ask-owner-missing")?.getAttribute("data-owner-tab-id")).toBe("orch-1");
    expect(q(stopped.host, "plan-ask-send")).toBeNull();
    expect(q(stopped.host, "plan-ask-expand")).toBeNull();
    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
  });

  it("shows a busy owner calmly, renders only this exchange as it streams, and expands to the full chat", async () => {
    const onOpenTab = vi.fn();
    const ui = (state: "idle" | "working") => (
      <PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab(state)} onOpenTab={onOpenTab} />
    );
    const { host, rerender } = await render(ui("working"));
    const paragraph = planBody(host).querySelector("p")!;
    await act(async () => paragraph.focus());
    await key(paragraph, "Enter");
    expect(q(host, "plan-ask-busy")).not.toBeNull();
    await typeQuestion(host, "Status?");
    await click(q(host, "plan-ask-send"));
    const askId = sendTrackedChatMessage.mock.calls[0][1] as string;

    expect(q(host, "plan-ask-question")!.textContent).toBe("Status?");
    expect(q(host, "plan-ask-reply")).toBeNull();
    expect(q(host, "plan-ask-waiting")).not.toBeNull();

    transcript.messages = [
      { id: "a0", type: "assistant", content: "Unrelated earlier answer" },
      { id: "u1", type: "user", content: "question text", correlationId: askId },
      { id: "a1", type: "assistant", content: "Partial" },
    ];
    await rerender(ui("working"));
    const streaming = Array.from(host.querySelectorAll('[data-testid="plan-ask-reply"]'));
    expect(streaming).toHaveLength(1);
    expect(streaming[0].textContent).toContain("Partial");

    transcript.messages = [
      ...transcript.messages.slice(0, 2),
      { id: "a1", type: "assistant", content: "Partial reply, now complete" },
      { id: "u2", type: "user", content: "a later unrelated question" },
      { id: "a2", type: "assistant", content: "Later unrelated reply" },
    ];
    await rerender(ui("idle"));
    const replies = Array.from(host.querySelectorAll('[data-testid="plan-ask-reply"]'));
    expect(replies).toHaveLength(1);
    expect(replies[0].textContent).toContain("Partial reply, now complete");
    expect(q(host, "plan-ask-exchange")!.textContent).not.toContain("Unrelated");
    expect(q(host, "plan-ask-waiting")).toBeNull();

    await click(q(host, "plan-ask-expand"));
    expect(onOpenTab).toHaveBeenCalledWith("orch-1");
    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);
  });
});

describe("Spec reader paragraph Ask", () => {
  it("offers the paragraph affordance and sends Spec references with UTF-16 offsets", async () => {
    const { host } = await render(<PlanSpecView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const scale = specBody(host, "sp_scale").querySelector("p")!;
    stubTop(scale, 64);
    await hover(scale);
    const trigger = q<HTMLElement>(host, "plan-ask-trigger")!;
    expect(trigger.dataset.askMode).toBe("paragraph");
    expect(trigger.style.transform).toBe("translateY(64px)");

    const body = specBody(host, "sp_readable");
    const node = textNode(body, "document — the reasoning");
    const phrase = "document — the reasoning";
    const start = node.data.indexOf(phrase);
    await select(node, start, node, start + phrase.length);
    await click(q(host, "plan-ask-trigger"));
    await typeQuestion(host, "Which reasoning?");
    await click(q(host, "plan-ask-send"));

    const source = specSource("sp_readable");
    expect(sentReference()).toMatchObject({
      plan_id: "pl_ask",
      document: "spec",
      spec_id: PERF_SPEC_ID,
      section_id: "sp_readable",
      mode: "selection",
      quote: phrase,
      source_start: source.indexOf(phrase),
      source_end: source.indexOf(phrase) + phrase.length,
    });
  });

  it("refuses a selection that spans two sections instead of guessing a reference", async () => {
    const { host } = await render(<PlanSpecView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const from = textNode(specBody(host, "sp_scale"), "The board");
    const to = textNode(specBody(host, "sp_zoom_nav"), "Zooming");
    await select(from, 4, to, 7);
    await click(q(host, "plan-ask-trigger"));
    expect(q(host, "plan-ask-unmapped")).not.toBeNull();
    expect(q(host, "plan-ask-input")).toBeNull();
    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
  });
});

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RichMarkdown } from "@codeterm/chat-ui/MarkdownBody";
import type { ChatSendResult } from "../../../../gen/ChatSendResult";
import type { ChatSendState } from "../../../../gen/ChatSendState";
import type { Tab } from "../../../lib/types";
import { forgetChatSendsInMemory, observeDeliveryStatus, recordChatSendAdmissionFailure } from "../../Terminal/chatSendStore";
import type { ChatSendStatus } from "../../Terminal/chatSendStatus";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { PERF_SPEC_ID, PERF_SPEC_SECTIONS } from "../perfFixtureSections";
import type { PlanSectionView } from "../planProjection";
import type { Plan } from "../types";
import { PlanPlanView, PlanSpecView } from "./PlanDocumentView";
import { PlanParagraphAsk } from "./PlanParagraphAsk";

const sendTrackedChatMessage = vi.hoisted(() => vi.fn());
const isProvenAdmissionRefusal = vi.hoisted(() => vi.fn(() => false));
const mux = vi.hoisted(() => ({ tabStatuses: {} as Record<string, "Working" | "Idle"> }));
vi.mock("../../../store/multiplexer/hooks", () => ({ useMuxSelector: (selector: (state: typeof mux) => unknown) => selector(mux) }));
const transcript = vi.hoisted(() => ({ messages: [] as Array<Record<string, unknown>> }));

vi.mock("../../Terminal/chatSendStatus", () => ({ sendTrackedChatMessage, isProvenAdmissionRefusal }));
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
const REVIEW_SPEC_BODY = "The **signal** phrase repeats: second **signal** phrase.";
const REVIEW_SPEC_SECTION = {
  id: "sp_repeated",
  title: "Repeated phrase",
  body: REVIEW_SPEC_BODY,
  subsections: [],
} as unknown as PlanSectionView;

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
  mux.tabStatuses["orch-1"] = state === "working" ? "Working" : "Idle";
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

function reviewTabs(): Record<string, Tab> {
  return {
    ...ownerTab(),
    "planner-1": {
      id: "planner-1",
      title: "Planning",
      agent_type: "fixture-agent",
      agent_session_id: "planner-session",
      agent_state: { state: "idle" },
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
    await Promise.resolve();
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

let sendSequence = 0;
let queuedSendStates: ChatSendState[] = [];

function deliveryStatus(tabId: string, clientToken: string, state: ChatSendState): ChatSendStatus {
  sendSequence += 1;
  return {
    tab_id: tabId,
    send_id: `plan-send-${sendSequence}`,
    correlationId: clientToken,
    state,
    error: null,
    ledger_id: "plan-review-test-ledger",
    sequence: sendSequence,
    retained_from: 1,
  };
}

function queueSendStates(...states: ChatSendState[]) {
  queuedSendStates = [...states];
  sendTrackedChatMessage.mockImplementation(async (tabId: string, clientToken: string) => {
    const status = deliveryStatus(tabId, clientToken, queuedSendStates.shift() ?? "delivered");
    observeDeliveryStatus(status);
    return status as ChatSendResult;
  });
}

function textNodes(root: Node): Text[] {
  const found: Text[] = [];
  const visit = (node: Node) => {
    if (node.nodeType === 3) found.push(node as Text);
    node.childNodes.forEach(visit);
  };
  visit(root);
  return found;
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

function repeatedSpecAskElement(plan = ASK_PLAN, tabs = ownerTab(), onOpenTab?: (tabId: string) => void) {
  return (
    <PlanParagraphAsk kind="spec" plan={plan} sections={[REVIEW_SPEC_SECTION]} tabs={tabs} onOpenTab={onOpenTab}>
      <div data-plan-ask-body="" data-section-id={REVIEW_SPEC_SECTION.id}>
        <RichMarkdown text={REVIEW_SPEC_BODY} sourcePositions />
      </div>
    </PlanParagraphAsk>
  );
}

async function renderRepeatedSpecAsk(plan = ASK_PLAN, tabs = ownerTab(), onOpenTab?: (tabId: string) => void) {
  return render(repeatedSpecAskElement(plan, tabs, onOpenTab));
}

async function selectRepeatedSpecPhrase(host: HTMLElement) {
  const body = host.querySelector<HTMLElement>("[data-plan-ask-body]")!;
  const phrases = textNodes(body).filter((node) => node.data === "signal");
  expect(phrases).toHaveLength(2);
  const secondPhrase = phrases[1];
  await select(secondPhrase, 0, secondPhrase, secondPhrase.data.length);
  await click(q(host, "plan-ask-trigger"));
}

beforeEach(() => {
  setReducedMotion(false);
  transcript.messages = [];
  forgetChatSendsInMemory();
  sendSequence = 0;
  sendTrackedChatMessage.mockReset();
  isProvenAdmissionRefusal.mockReset();
  isProvenAdmissionRefusal.mockReturnValue(false);
  queueSendStates();
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
    expect(body.contains(trigger)).toBe(false);

    await hover(item);
    expect(q(host, "plan-ask-trigger")).toBe(trigger);
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
    const [tabId, clientToken, text] = sendTrackedChatMessage.mock.calls[0] as [string, string, string];
    expect(tabId).toBe("orch-1");
    expect(text).toContain("Why login first?");
    expect(sentReference().ask_id).not.toBe(clientToken);
    expect(sentReference()).toEqual({
      ask_id: sentReference().ask_id,
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
    const askCorrelationId = sendTrackedChatMessage.mock.calls[0][1] as string;

    expect(q(host, "plan-ask-question")!.textContent).toBe("Status?");
    expect(q(host, "plan-ask-reply")).toBeNull();
    expect(q(host, "plan-ask-waiting")).not.toBeNull();

    transcript.messages = [
      { id: "a0", type: "assistant", content: "Unrelated earlier answer" },
      { id: "u1", type: "user", content: "question text", correlationId: askCorrelationId },
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
    expect(q(host, "plan-ask-type-selector")).toBeNull();
    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
  });

  it.each([
    { action: "comment", label: "Comment", body: "This repeated signal is unclear." },
    { action: "request_change", label: "Request change", body: "Clarify the second signal." },
  ] as const)("sends one labelled $label review for the exact formatted occurrence", async ({ action, label, body: note }) => {
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, `plan-ask-type-${action}`));
    await typeQuestion(host, note);
    await click(q(host, "plan-ask-send"));

    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);
    const [tabId, clientToken, text] = sendTrackedChatMessage.mock.calls[0] as [string, string, string];
    const reviewId = sentReference().review_id;
    expect(tabId).toBe("orch-1");
    expect(reviewId).not.toBe(clientToken);
    expect(text).toContain(`One-item plan review: ${label}`);
    expect(q(host, "plan-review-sent")?.textContent).toContain(action === "request_change" ? "Change request sent" : "Comment sent");
    expect(sentReference()).toEqual({
      review_id: reviewId,
      kind: "plan_review",
      plan_id: "pl_ask",
      document: "spec",
      spec_id: PERF_SPEC_ID,
      item_count: 1,
      items: [{
        action,
        body: note,
        mode: "selection",
        section_id: REVIEW_SPEC_SECTION.id,
        section_title: REVIEW_SPEC_SECTION.title,
        source_start: REVIEW_SPEC_BODY.lastIndexOf("signal"),
        source_end: REVIEW_SPEC_BODY.lastIndexOf("signal") + "signal".length,
        rendered_start: "The signal phrase repeats: second ".length,
        rendered_end: "The signal phrase repeats: second signal".length,
        offset_unit: "utf16_code_unit",
        quote: "signal",
      }],
    });
  });

  it("keeps Ask with the Orchestrator and routes reviews to a live Planner", async () => {
    const planWithPlanner: Plan = {
      ...ASK_PLAN,
      planner: { kind: "tab", tab_id: "planner-1", session_id: "planner-session", title: "Planning" },
    };

    const ask = await renderRepeatedSpecAsk(planWithPlanner, reviewTabs());
    await selectRepeatedSpecPhrase(ask.host);
    await typeQuestion(ask.host, "Ask the Orchestrator.");
    await click(q(ask.host, "plan-ask-send"));
    expect(sendTrackedChatMessage.mock.calls[0][0]).toBe("orch-1");

    sendTrackedChatMessage.mockClear();
    const review = await renderRepeatedSpecAsk(planWithPlanner, reviewTabs());
    await selectRepeatedSpecPhrase(review.host);
    await click(q(review.host, "plan-ask-type-comment"));
    await typeQuestion(review.host, "Send the review to the Planner.");
    await click(q(review.host, "plan-ask-send"));
    expect(sendTrackedChatMessage.mock.calls[0][0]).toBe("planner-1");
  });

  it.each([
    ["no Planner", null],
    ["a waived Planner", { kind: "waived", note: "solo planning" }],
  ] as const)("falls back to the Orchestrator when there is %s", async (_name, planner) => {
    const plan = { ...ASK_PLAN, planner } as Plan;
    const { host } = await renderRepeatedSpecAsk(plan, ownerTab());
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-request_change"));
    await typeQuestion(host, "Use the recorded Orchestrator.");
    await click(q(host, "plan-ask-send"));
    expect(sendTrackedChatMessage.mock.calls[0][0]).toBe("orch-1");
  });

  it("keeps review submission unavailable when neither recipient is recorded", async () => {
    const unowned: Plan = {
      ...ASK_PLAN,
      owner: { tab_id: null, session_id: null, title: null },
      planner: null,
    };
    const { host } = await renderRepeatedSpecAsk(unowned);
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));

    expect(q(host, "plan-ask-owner-missing")).not.toBeNull();
    expect(q(host, "plan-ask-input")).toBeNull();
    expect(q(host, "plan-ask-send")).toBeNull();
    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
  });

  it("does not submit to a hidden-only Planner binding", async () => {
    const plan: Plan = {
      ...ASK_PLAN,
      owner: { tab_id: null, session_id: null, title: null },
      planner: { kind: "tab", tab_id: "planner-1", session_id: "planner-session", title: "Planning" },
    };
    const hiddenPlanner = { ...reviewTabs()["planner-1"], hidden: true } as Tab;
    const { host } = await renderRepeatedSpecAsk(plan, { "planner-1": hiddenPlanner });
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));

    expect(q(host, "plan-ask-owner-missing")?.getAttribute("data-owner-tab-id")).toBe("planner-1");
    expect(q(host, "plan-ask-input")).toBeNull();
    expect(q(host, "plan-ask-send")).toBeNull();
    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
  });

  it("falls back from a hidden Planner to the live Orchestrator", async () => {
    const plan: Plan = {
      ...ASK_PLAN,
      planner: { kind: "tab", tab_id: "planner-1", session_id: "planner-session", title: "Planning" },
    };
    const tabs = reviewTabs();
    tabs["planner-1"] = { ...tabs["planner-1"], hidden: true } as Tab;
    const { host } = await renderRepeatedSpecAsk(plan, tabs);
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Use the visible fallback owner.");
    await click(q(host, "plan-ask-send"));

    expect(sendTrackedChatMessage.mock.calls[0][0]).toBe("orch-1");
  });

  it("updates a held review to failed and retries with a fresh token but the same review id", async () => {
    queueSendStates("held", "delivered");
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-request_change"));
    await typeQuestion(host, "Clarify the selected passage.");
    await click(q(host, "plan-ask-send"));

    const originalToken = sendTrackedChatMessage.mock.calls[0][1] as string;
    const reviewId = sentReference().review_id;
    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe("held");
    expect(q(host, "plan-ask-send")).toBeNull();

    await act(async () => observeDeliveryStatus(deliveryStatus("orch-1", originalToken, "failed")));
    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe("failed");
    expect(q(host, "plan-ask-send")?.textContent).toBe("Retry");
    expect(q(host, "plan-review-sent")?.textContent).toContain("Change request was not sent");

    await click(q(host, "plan-ask-send"));
    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(2);
    expect(sendTrackedChatMessage.mock.calls[1][1]).not.toBe(originalToken);
    expect(sentReference(1)).toMatchObject({ review_id: reviewId, item_count: 1 });
    expect(q(host, "plan-review-sent")?.textContent).toContain("Change request sent");
  });

  it("updates a held review to delivered through the send ledger", async () => {
    queueSendStates("held");
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Deliver this held comment once.");
    await click(q(host, "plan-ask-send"));

    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe("held");
    expect(q(host, "plan-ask-send")).toBeNull();
    expect(q(host, "plan-ask-dismiss")).not.toBeNull();

    await act(async () => observeDeliveryStatus(deliveryStatus(
      "orch-1", sendTrackedChatMessage.mock.calls[0][1] as string, "delivered",
    )));
    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe("delivered");
    expect(q(host, "plan-review-sent")?.textContent).toContain("Comment sent");
    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);
  });

  it("keeps delivery status and retry bound to the submitted recipient after live resolution changes", async () => {
    queueSendStates("failed", "delivered");
    const plan: Plan = {
      ...ASK_PLAN,
      planner: { kind: "tab", tab_id: "planner-1", session_id: "planner-session", title: "Planning" },
    };
    const onOpenTab = vi.fn();
    const { host, rerender } = await renderRepeatedSpecAsk(plan, reviewTabs(), onOpenTab);
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Keep the original recipient.");
    await click(q(host, "plan-ask-send"));

    const originalToken = sendTrackedChatMessage.mock.calls[0][1] as string;
    const reviewId = sentReference().review_id;
    expect(sendTrackedChatMessage.mock.calls[0][0]).toBe("planner-1");
    expect(q(host, "plan-review-sent")?.textContent).toContain("to Planning");

    await rerender(repeatedSpecAskElement(plan, ownerTab(), onOpenTab));
    expect(q(host, "plan-review-sent")?.textContent).toContain("to Planning");
    await click(q(host, "plan-ask-expand"));
    expect(onOpenTab).toHaveBeenCalledWith("planner-1");
    await click(q(host, "plan-ask-send"));

    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(2);
    expect(sendTrackedChatMessage.mock.calls[1][0]).toBe("planner-1");
    expect(sendTrackedChatMessage.mock.calls[1][1]).not.toBe(originalToken);
    expect(sentReference(1).review_id).toBe(reviewId);
    expect(q(host, "plan-review-sent")?.textContent).toContain("to Planning");
  });

  it.each(["written", "submit_unconfirmed"] as const)("does not retry a review with %s delivery", async (state) => {
    queueSendStates(state);
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Keep this to one delivery attempt.");
    await click(q(host, "plan-ask-send"));

    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe(state);
    expect(q(host, "plan-ask-send")).toBeNull();
    expect(q(host, "plan-ask-dismiss")).not.toBeNull();
    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);
  });

  it("treats an ambiguous send rejection as unconfirmed and lets the user dismiss it", async () => {
    sendTrackedChatMessage.mockImplementationOnce(async (tabId: string, clientToken: string) => {
      recordChatSendAdmissionFailure(tabId, clientToken, false, new Error("connection lost"));
      throw new Error("connection lost");
    });
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Do not replay an uncertain review.");
    await click(q(host, "plan-ask-send"));

    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe("submit_unconfirmed");
    expect(q(host, "plan-ask-send")).toBeNull();
    expect(q(host, "plan-ask-dismiss")).not.toBeNull();
    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);
  });

  it("retries a proven admission refusal with a fresh token and stable review id", async () => {
    sendTrackedChatMessage.mockRejectedValueOnce(new Error("known admission refusal"));
    isProvenAdmissionRefusal.mockReturnValueOnce(true);
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Retry only this proven refusal.");
    await click(q(host, "plan-ask-send"));

    expect(q(host, "plan-review-sent")?.getAttribute("data-send-state")).toBe("failed");
    const token = sendTrackedChatMessage.mock.calls[0][1] as string;
    const reviewId = sentReference().review_id;
    await click(q(host, "plan-ask-send"));
    expect(sendTrackedChatMessage.mock.calls[1][1]).not.toBe(token);
    expect(sentReference(1).review_id).toBe(reviewId);
  });

  it("switches Cancel to Dismiss while a tracked review is still sending", async () => {
    let resolveSend: ((value: { state: "delivered" }) => void) | undefined;
    sendTrackedChatMessage.mockImplementation(() => new Promise((resolve) => { resolveSend = resolve; }));
    const { host } = await renderRepeatedSpecAsk();
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Dismiss without cancelling the in-flight send.");
    await click(q(host, "plan-ask-send"));

    expect(q(host, "plan-ask-cancel")?.textContent).toBe("Dismiss");
    expect(q(host, "plan-ask-cancel")?.getAttribute("aria-label")).toBe("Dismiss");
    expect(q(host, "plan-ask-close")?.getAttribute("aria-label")).toBe("Dismiss");
    await click(q(host, "plan-ask-cancel"));
    expect(q(host, "plan-ask-popover")).toBeNull();
    expect(sendTrackedChatMessage).toHaveBeenCalledTimes(1);

    await act(async () => resolveSend?.({ state: "delivered" }));
  });

  it("cancels a selected passage comment without sending or changing the Spec", async () => {
    const { host } = await renderRepeatedSpecAsk();
    const before = host.querySelector("[data-plan-ask-body]")?.textContent;
    await selectRepeatedSpecPhrase(host);
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "This should stay local.");
    await click(q(host, "plan-ask-cancel"));

    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
    expect(host.querySelector("[data-plan-ask-body]")?.textContent).toBe(before);
    expect(REVIEW_SPEC_SECTION.body).toBe(REVIEW_SPEC_BODY);
  });

  it("does not offer review submission for a selected passage without a recorded owner", async () => {
    const unowned = { ...ASK_PLAN, owner: { tab_id: null, session_id: null, title: null } };
    const { host } = await renderRepeatedSpecAsk(unowned);
    await selectRepeatedSpecPhrase(host);

    expect(q(host, "plan-ask-owner-missing")).not.toBeNull();
    expect(q(host, "plan-ask-type-selector")).not.toBeNull();
    expect(q(host, "plan-ask-input")).toBeNull();
    expect(sendTrackedChatMessage).not.toHaveBeenCalled();
  });

  it("uses the full Spec document for review references and keeps reviews selection-only", async () => {
    const { host } = await render(<PlanSpecView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const paragraph = specBody(host, "sp_readable").querySelector("p")!;
    await hover(paragraph);
    await click(q(host, "plan-ask-trigger"));
    expect(q(host, "plan-ask-type-selector")).toBeNull();
    await click(q(host, "plan-ask-cancel"));

    const node = textNode(specBody(host, "sp_readable"), "document — the reasoning");
    const phrase = "document — the reasoning";
    const start = node.data.indexOf(phrase);
    await select(node, start, node, start + phrase.length);
    await click(q(host, "plan-ask-trigger"));
    await click(q(host, "plan-ask-type-request_change"));
    await typeQuestion(host, "Make the Spec rationale explicit.");
    await click(q(host, "plan-ask-send"));

    const source = specSource("sp_readable");
    expect(sendTrackedChatMessage.mock.calls[0][0]).toBe("orch-1");
    expect(sentReference()).toMatchObject({
      kind: "plan_review",
      plan_id: ASK_PLAN.id,
      document: "spec",
      spec_id: PERF_SPEC_ID,
      item_count: 1,
      items: [{
        action: "request_change",
        section_id: "sp_readable",
        section_title: "Plans read like documents",
        source_start: source.indexOf(phrase),
        source_end: source.indexOf(phrase) + phrase.length,
        quote: phrase,
      }],
    });
  });

  it("sends a Plan document reference with its section and selected source offsets", async () => {
    const { host } = await render(<PlanPlanView mode="focus" planId={ASK_PLAN.id} plans={[ASK_PLAN]} tabs={ownerTab()} />);
    const node = textNode(planBody(host), "Rotate the signing key");
    const phrase = "signing key";
    const start = node.data.indexOf(phrase);
    await select(node, start, node, start + phrase.length);
    await click(q(host, "plan-ask-trigger"));
    await click(q(host, "plan-ask-type-comment"));
    await typeQuestion(host, "Keep the signing key rotation explicit.");
    await click(q(host, "plan-ask-send"));

    expect(sentReference()).toMatchObject({
      document: "plan",
      spec_id: null,
      items: [{
        action: "comment",
        section_id: "sec_ask",
        section_title: "Rollout",
        quote: phrase,
        source_start: PLAN_BODY.indexOf(phrase),
        source_end: PLAN_BODY.indexOf(phrase) + phrase.length,
      }],
    });
  });
});

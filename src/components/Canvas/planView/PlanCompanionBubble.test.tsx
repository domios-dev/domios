// Run: npx tsx src/components/Canvas/planView/PlanCompanionBubble.test.tsx

import { act, type ReactElement } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../../lib/bridge";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Tab } from "../../../lib/types";
import type { Plan, PlannerRef } from "../types";
import { PlanCompanionBubble } from "./PlanCompanionBubble";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/plan-companion" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Node", "KeyboardEvent"] as const) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
  if (!window.matchMedia) {
    (window as unknown as { matchMedia: unknown }).matchMedia = () => ({
      matches: false,
      addEventListener() {},
      removeEventListener() {},
    });
  }
}

setupDom();
__setInvokeForTest(async () => undefined as never);
__setListenForTest(async () => () => {});

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function tab(overrides: Partial<Tab>): Tab {
  return { id: "x", title: "Tab", agent_type: "claude", ...overrides } as Tab;
}

function plan(planner: PlannerRef | null): Plan {
  return { ...FIXTURE_PLAN, id: "plan-alpha", planner };
}

function renderChatStub({ tabId }: { tabId: string; chatBackend: boolean }) {
  return <div data-testid="plan-companion-chat" data-tab-id={tabId} />;
}

const LIVE_TABS: Record<string, Tab> = {
  "planner-1": tab({ id: "planner-1", title: "Planner", agent_state: { state: "working" } }),
  "orch-1": tab({ id: "orch-1", title: "Orchestrator", agent_state: { state: "idle" } }),
};

test("renders planner bubble when planner Tab is live", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan({ kind: "tab", tab_id: "planner-1" })]} tabs={LIVE_TABS} renderChat={renderChatStub} />,
  );
  const bubble = host.querySelector('[data-testid="plan-companion-bubble"]');
  assert(bubble, "bubble should render");
  assert(bubble.getAttribute("data-companion-kind") === "planner", "should resolve planner");
  await act(async () => root.unmount());
});

test("falls back to orchestrator bubble when planner is dead", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan({ kind: "tab", tab_id: "gone" })]} tabs={LIVE_TABS} renderChat={renderChatStub} />,
  );
  const bubble = host.querySelector('[data-testid="plan-companion-bubble"]');
  assert(bubble?.getAttribute("data-companion-kind") === "orchestrator", "should fall back to orchestrator");
  await act(async () => root.unmount());
});

test("hidden when nothing live resolves", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan(null)]} tabs={{ "orch-1": tab({ id: "orch-1", agent_type: undefined }) }} renderChat={renderChatStub} />,
  );
  assert(!host.querySelector('[data-testid="plan-companion-bubble"]'), "bubble should be hidden");
  await act(async () => root.unmount());
});

test("click opens popover, close button dismisses it", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan({ kind: "tab", tab_id: "planner-1" })]} tabs={LIVE_TABS} renderChat={renderChatStub} />,
  );
  const bubble = host.querySelector('[data-testid="plan-companion-bubble"]') as HTMLButtonElement;
  await act(async () => bubble.click());
  assert(host.querySelector('[data-testid="plan-companion-popover"]'), "popover should open");
  const close = host.querySelector('[data-testid="plan-companion-close"]') as HTMLButtonElement;
  await act(async () => close.click());
  assert(!host.querySelector('[data-testid="plan-companion-popover"]'), "popover should close");
  await act(async () => root.unmount());
});

test("Escape closes an open popover", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan({ kind: "tab", tab_id: "planner-1" })]} tabs={LIVE_TABS} renderChat={renderChatStub} />,
  );
  const bubble = host.querySelector('[data-testid="plan-companion-bubble"]') as HTMLButtonElement;
  await act(async () => bubble.click());
  assert(host.querySelector('[data-testid="plan-companion-popover"]'), "popover should open");
  await act(async () => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  });
  assert(!host.querySelector('[data-testid="plan-companion-popover"]'), "Escape should close popover");
  await act(async () => root.unmount());
});

test("bubble face is a chat icon with an edge presence dot, name in tooltip", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan({ kind: "tab", tab_id: "planner-1" })]} tabs={LIVE_TABS} renderChat={renderChatStub} />,
  );
  const bubble = host.querySelector('[data-testid="plan-companion-bubble"]') as HTMLButtonElement;
  assert(bubble, "bubble renders");
  // Chat-bubble face, not an avatar: a MessageCircle svg lives inside the button.
  const icon = bubble.querySelector("svg");
  assert(icon, "chat icon svg present in button face");
  assert(
    (icon.getAttribute("class") ?? "").includes("message-circle"),
    `expected message-circle icon, got class: ${icon.getAttribute("class")}`,
  );
  // Presence is an edge indicator, not a full avatar.
  const presence = bubble.querySelector('[data-testid="plan-companion-presence"]');
  assert(presence, "edge presence dot present on the button");
  // Name lives in the tooltip (title), not painted on the face.
  const title = bubble.getAttribute("title") ?? "";
  assert(title.includes("Planner"), `name should be in tooltip, got: ${title}`);
  await act(async () => root.unmount());
});

test("working companion presence uses quiet micro activity dots", async () => {
  const { host, root } = await render(
    <PlanCompanionBubble plans={[plan({ kind: "tab", tab_id: "planner-1" })]} tabs={LIVE_TABS} renderChat={renderChatStub} />,
  );
  const dots = host.querySelector('[data-testid="plan-companion-presence"].activity-dots');
  assert(dots?.classList.contains("activity-dots--micro"), "working presence uses micro dots");
  assert(dots?.classList.contains("activity-dots--muted"), "working presence uses the quiet tone");
  const activityCss = readFileSync(new URL("../../../styles/agent-state.css", import.meta.url), "utf8");
  assert(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.activity-dots i\s*\{[^}]*animation:\s*none !important;/s.test(activityCss), "activity dots retain a static reduced-motion cue");
  await act(async () => root.unmount());
});

async function run() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (err) {
      failed++;
      console.error(`  ✗ ${name}: ${(err as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n${failed} of ${tests.length} PlanCompanionBubble test(s) failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} PlanCompanionBubble test(s) green`);
}

run();

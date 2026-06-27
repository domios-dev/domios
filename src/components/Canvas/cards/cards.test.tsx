// Run: npx tsx src/components/Canvas/cards/cards.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  MilestoneBubble,
  SectionNode,
  StrideSnake,
  TaskCard,
} from "./index";
import {
  FIXTURE_DRIFT,
  FIXTURE_MILESTONE,
  FIXTURE_NOT_MINE_DRIFT,
  FIXTURE_PLAN,
  FIXTURE_TAG_FACETS,
  FIXTURE_TASK,
} from "./fixtures.testdata";
import type { PlanWorkspaceScope } from "../planWorkspaceScope";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function isVisible(el: Element | null): boolean {
  if (!el) return false;
  let node: Element | null = el;
  while (node) {
    const html = node as HTMLElement;
    const cs = html.style;
    if (cs.display === "none" || cs.visibility === "hidden") return false;
    if (cs.opacity && Number(cs.opacity) < 0.05) return false;
    node = node.parentElement;
  }
  return true;
}

function taskWithStrides() {
  return { ...FIXTURE_TASK, milestones: [FIXTURE_MILESTONE] };
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/canvas-cards" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLPreElement", { value: window.HTMLPreElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

const cardStyle = { position: "absolute" as const, left: 0, top: 0, width: 300 };

test("PLAN SectionNode renders title, section rows, output dots, and 50% coverage", async () => {
  const { host, root } = await render(
    <SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />,
  );
  assert(host.textContent?.includes("Auth rollout"), "plan title visible");
  assert(host.querySelector('[data-testid="canvas-plan-coverage-pct"]')?.textContent?.includes("50%"), "coverage pct");
  const rows = host.querySelectorAll('[data-testid="canvas-plan-row"]');
  assert(rows.length === 2, `two section rows (parent + subsection), got ${rows.length}`);
  assert(host.querySelectorAll('[data-testid="canvas-plan-output"]').length === 2, "an output dot per section row");
  await act(async () => root.unmount());
});

test("PLAN SectionNode: owner chip hidden for an all-empty OrchestratorRef (no 'unknown')", async () => {
  const empty = { ...FIXTURE_PLAN, owner: { pane_id: "", session_id: "", title: "" } };
  const { host, root } = await render(<SectionNode variant="plan" plan={empty} style={cardStyle} world="W3" />);
  assert(!host.querySelector('[data-testid="canvas-plan-owner"]'), "owner chip hidden when unowned");
  assert(!host.textContent?.toLowerCase().includes("unknown"), "no literal 'unknown' rendered");
  await act(async () => root.unmount());

  const sentinel = { ...FIXTURE_PLAN, owner: { pane_id: "unknown", session_id: null, title: "unknown" } };
  const s = await render(<SectionNode variant="plan" plan={sentinel} style={cardStyle} world="W3" />);
  assert(!s.host.querySelector('[data-testid="canvas-plan-owner"]'), "sentinel 'unknown' owner hides chip");
  await act(async () => s.root.unmount());

  const owned = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />);
  assert(owned.host.querySelector('[data-testid="canvas-plan-owner"]')?.textContent?.includes("Orchestrator"), "owner chip shows title when owned");
  await act(async () => owned.root.unmount());
});

test("PLAN SectionNode: cross-workspace scope badge includes owner workspace", async () => {
  const workspaceScope: PlanWorkspaceScope = {
    ownerWorkspaceId: "ws-owner",
    ownerWorkspaceLabel: "Canvas Hub",
    participantWorkspaceLabels: ["Canvas Hub", "API Lane"],
    isCrossWorkspace: true,
  };
  const { host, root } = await render(
    <SectionNode variant="plan" plan={FIXTURE_PLAN} workspaceScope={workspaceScope} style={cardStyle} world="W3" />,
  );
  const owner = host.querySelector('[data-testid="canvas-plan-owner"]');
  assert(owner?.textContent?.includes("Orchestrator"), "owner pane still visible");
  assert(owner?.textContent?.includes("Canvas Hub"), `owner workspace visible, got ${owner?.textContent}`);
  const badge = host.querySelector('[data-testid="canvas-plan-cross-workspace"]');
  assert(badge?.textContent?.toLowerCase().includes("cross-workspace"), `cross-workspace badge visible, got ${badge?.textContent}`);
  assert((badge as HTMLElement | null)?.title?.includes("API Lane"), "participant workspace labels in tooltip");
  await act(async () => root.unmount());
});

test("PLAN SectionNode: uncovered section row pops with uncovered glyph", async () => {
  const { host, root } = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />);
  const oauth = host.querySelector('[data-section-id="sec-auth-oauth"]');
  assert(oauth?.getAttribute("data-row-glyph") === "uncovered", "uncovered subsection pops");
  const covered = host.querySelector('[data-section-id="sec-auth"]');
  assert(covered?.getAttribute("data-row-glyph") === "progress", "covered row is in-progress");
  await act(async () => root.unmount());
});

test("PLAN SectionNode: spec-reference bubbles render per section and fly to spec-section", async () => {
  const plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-x",
    spec_links: [
      { spec_id: "spec-x", spec_section_id: "sp-1", plan_section_id: "sec-auth" },
      { spec_id: "spec-x", spec_section_id: "sp-2", plan_section_id: "sec-auth" },
      // duplicate must collapse to one bubble
      { spec_id: "spec-x", spec_section_id: "sp-1", plan_section_id: "sec-auth" },
    ],
  };
  const specTitleById = new Map([["sp-1", "Sign-in requirement"], ["sp-2", "Token lifetime"]]);
  const { host, root } = await render(
    <SectionNode variant="plan" plan={plan} style={cardStyle} world="W3" specTitleById={specTitleById} />,
  );
  const authRow = host.querySelector('[data-section-id="sec-auth"]');
  const bubbles = authRow?.querySelectorAll('[data-testid="canvas-plan-spec-ref"]');
  assert(bubbles && bubbles.length === 2, `two spec bubbles after dedup, got ${bubbles?.length}`);
  const first = bubbles![0];
  assert(first.getAttribute("data-fly-to-kind") === "spec-section", "bubble carries spec-section fly kind");
  assert(first.getAttribute("data-fly-to-id") === "sp-1", "bubble carries spec section id");
  assert(host.textContent?.includes("Sign-in requirement"), "bubble shows resolved spec section title");
  // Plan body reads as a document at W2 (not just W3) — no clamp.
  assert(host.querySelector('[data-testid="canvas-plan-section-body"]'), "plan body renders as document");
  await act(async () => root.unmount());
});

test("PLAN SectionNode: full body revealed at W2 (document, not clamped excerpt)", async () => {
  const longBody = Array.from({ length: 12 }, (_, i) => `Paragraph ${i} of the plan section body.`).join("\n\n");
  const plan = { ...FIXTURE_PLAN, sections: [{ ...FIXTURE_PLAN.sections[0], body: longBody, subsections: [] }] };
  const { host, root } = await render(<SectionNode variant="plan" plan={plan} style={cardStyle} world="W2" />);
  const body = host.querySelector('[data-testid="canvas-plan-section-body"]');
  assert(body, "plan body present at W2");
  assert(host.textContent?.includes("Paragraph 0"), "first paragraph rendered at W2");
  assert(host.textContent?.includes("Paragraph 11"), "last paragraph rendered (full body, nothing clamped)");
  await act(async () => root.unmount());
});

test("SPEC SectionNode: titles far (index), full body prose on zoom (content-LOD)", async () => {
  const far = await render(<SectionNode variant="spec" plan={FIXTURE_PLAN} style={{ ...cardStyle, height: 300 }} world="W1" />);
  assert(far.host.querySelector('[data-testid="canvas-spec-block"]'), "spec block present far");
  assert(!far.host.querySelector('[data-testid="canvas-spec-section-body"]'), "no prose far (index only)");
  await act(async () => far.root.unmount());

  const near = await render(<SectionNode variant="spec" plan={FIXTURE_PLAN} style={{ ...cardStyle, height: 300 }} world="W3" />);
  assert(near.host.querySelector('[data-testid="canvas-spec-section-body"]'), "prose revealed near");
  assert(near.host.textContent?.includes("sign in"), "section body markdown text visible near");
  await act(async () => near.root.unmount());
});

test("TaskCard subtasks: parent shows done/total count; child shows parent name", async () => {
  const parent = { ...FIXTURE_TASK, id: "task_parent", name: "Epic parent" };
  const child = { ...FIXTURE_TASK, id: "task_child", name: "Subtask", parent_task_id: "task_parent" };
  const parentCard = await render(<TaskCard task={parent} childCount={3} childDone={1} style={cardStyle} world="W3" />);
  const count = parentCard.host.querySelector('[data-testid="canvas-task-subtask-count"]');
  assert(count?.textContent?.includes("1/3"), `expected 1/3 subtask progress, got ${count?.textContent}`);
  await act(async () => parentCard.root.unmount());

  const childCard = await render(<TaskCard task={child} parentTaskName="Epic parent" style={cardStyle} world="W3" />);
  const parentBadge = childCard.host.querySelector('[data-testid="canvas-task-parent"]');
  assert(parentBadge?.textContent?.includes("Epic parent"), `parent badge missing, got ${parentBadge?.textContent}`);
  await act(async () => childCard.root.unmount());
});

test("TaskCard freshness: compact relative stamp from status/updated time", async () => {
  const now = Date.now();
  const fresh = { ...FIXTURE_TASK, status_changed_at: now - 5 * 60_000 };
  const { host, root } = await render(<TaskCard task={fresh} style={cardStyle} world="W3" />);
  const el = host.querySelector('[data-testid="canvas-task-freshness"]');
  assert(el, "freshness counter rendered");
  assert(el.textContent?.includes("5m"), `expected 5m freshness, got ${el.textContent}`);
  assert((el.querySelector("span") as HTMLElement | null)?.title?.length, "absolute time title present");
  await act(async () => root.unmount());

  const far = await render(<TaskCard task={fresh} style={cardStyle} world="W0" />);
  const farEl = far.host.querySelector('[data-testid="canvas-task-freshness"]');
  assert(farEl?.textContent?.includes("5m"), `expected W0 freshness, got ${farEl?.textContent}`);
  await act(async () => far.root.unmount());
});

test("TaskCard freshness: absent when no timestamp fields", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={cardStyle} world="W3" />);
  assert(!host.querySelector('[data-testid="canvas-task-freshness"]'), "no freshness without stamps");
  await act(async () => root.unmount());
});

test("TaskCard (near/full): name, model chip, status, and markdown body", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={cardStyle} world="W3" />);
  assert(host.querySelector('[data-testid="canvas-task-name"]')?.textContent === "Dante", "task name");
  assert(host.querySelector('[data-testid="canvas-task-model"]')?.textContent === "claude:opus", "model chip");
  assert(host.querySelector('[data-testid="canvas-task-status"]')?.textContent === "90% done", "status label");
  assert(host.querySelector('[data-testid="canvas-task-body"]'), "task markdown body");
  await act(async () => root.unmount());
});

test("TaskCard pane link chip appears for assigned tasks and fires onOpenPane", async () => {
  let opened: string | null = null;
  const assigned = { ...FIXTURE_TASK, assigned_pane_id: "pane-42" };
  const { host, root } = await render(
    <TaskCard task={assigned} style={cardStyle} world="W3" onOpenPane={(id) => (opened = id)} />,
  );
  const chip = host.querySelector('[data-testid="canvas-task-pane-link"]') as HTMLButtonElement;
  assert(chip, "pane link chip rendered");
  await act(async () => chip.click());
  assert(opened === "pane-42", `onOpenPane fired with pane id, got ${opened}`);
  await act(async () => root.unmount());
});

test("TaskCard presence marker shows assignee focus and latest stride", async () => {
  const assigned = {
    ...FIXTURE_TASK,
    assigned_pane_id: "pane-42",
    milestones: [
      {
        ...FIXTURE_MILESTONE,
        from: { pane_id: "pane-42", session_id: null, title: "Puccini" },
        kind: "status_update",
      },
    ],
  };
  const { host, root } = await render(
    <TaskCard
      task={assigned}
      presence={{
        paneId: "pane-42",
        label: "Puccini",
        focus: "Implement presence chip",
        latest: { authorLabel: "Puccini", kindLabel: "status update" },
        agents: [
          { id: "pane-42", label: "Puccini", role: "backend-dev", color: "#4f7df0" },
          { id: "peer:claude-sonnet", label: "claude-sonnet", role: "peer", color: "#e8643c", isPeer: true },
        ],
        branch: "feat/login-endpoint",
      }}
      style={cardStyle}
      world="W3"
    />,
  );
  const marker = host.querySelector('[data-testid="canvas-task-presence"]') as HTMLElement | null;
  assert(marker, "presence marker rendered");
  assert(marker.getAttribute("data-pane-id") === "pane-42", "pane id attached for presence marker");
  assert(marker.textContent?.includes("Puccini"), `lead label visible, got ${marker.textContent}`);
  assert(host.querySelectorAll('[data-testid="canvas-agent-circle"]').length === 2, "both agent circles rendered");
  assert(host.querySelector('[data-testid="canvas-task-presence-branch"]')?.textContent?.includes("feat/login-endpoint"), "branch surfaced glanceably");
  assert(marker.textContent?.includes("status update"), `latest stride kind visible, got ${marker.textContent}`);
  assert(marker.getAttribute("title")?.includes("Implement presence chip"), "focus available in tooltip");
  await act(async () => root.unmount());
});

test("TaskCard drift badge renders actionable, muted not_mine, and absent states", async () => {
  const plain = await render(<TaskCard task={FIXTURE_TASK} style={cardStyle} world="W3" />);
  assert(!plain.host.querySelector('[data-testid="canvas-task-drift-badge"]'), "no badge without drift");
  await act(async () => plain.root.unmount());

  const active = await render(<TaskCard task={FIXTURE_TASK} drift={FIXTURE_DRIFT} style={cardStyle} world="W3" />);
  const activeBadge = active.host.querySelector('[data-testid="canvas-task-drift-badge"]') as HTMLElement | null;
  assert(activeBadge, "drift badge rendered");
  assert(activeBadge.getAttribute("data-drift-kind") === "in_progress_pane_gone", "actionable kind attached");
  assert(activeBadge.getAttribute("data-not-mine") === "false", "actionable drift is not muted");
  assert(activeBadge.getAttribute("title")?.includes(FIXTURE_DRIFT.suggested_fix), "tooltip includes suggested fix");
  await act(async () => active.root.unmount());

  const muted = await render(<TaskCard task={FIXTURE_TASK} drift={FIXTURE_NOT_MINE_DRIFT} style={cardStyle} world="W3" />);
  const mutedBadge = muted.host.querySelector('[data-testid="canvas-task-drift-badge"]') as HTMLElement | null;
  assert(mutedBadge, "not_mine drift badge rendered");
  assert(mutedBadge.getAttribute("data-not-mine") === "true", "not_mine drift marked muted");
  assert((mutedBadge.getAttribute("style") ?? "").includes("opacity: 0.55"), "not_mine drift uses muted opacity");
  await act(async () => muted.root.unmount());
});

test("TaskCard content-LOD: compact far (name+status, no body) → full near (body)", async () => {
  const far = await render(<TaskCard task={FIXTURE_TASK} tagFacets={[...FIXTURE_TAG_FACETS]} style={cardStyle} world="W0" />);
  assert(far.host.querySelector('[data-testid="canvas-task-name"]'), "name present far");
  assert(far.host.querySelector('[data-testid="canvas-task-status"]'), "status present far");
  assert(!far.host.querySelector('[data-testid="canvas-task-body"]'), "no full body far (compact)");
  assert(far.host.querySelector('[data-testid="canvas-task-tile-tags"]'), "tile tag payload far");
  assert(!far.host.querySelector('[data-testid="canvas-task-digest"]'), "digest hidden when tags substitute body");
  assert(far.host.querySelector('[data-testid="canvas-task-primary-facet"]')?.textContent?.includes("Navigation"), "primary facet chip far");
  await act(async () => far.root.unmount());

  const near = await render(<TaskCard task={FIXTURE_TASK} tagFacets={[...FIXTURE_TAG_FACETS]} style={cardStyle} world="W3" />);
  assert(near.host.querySelector('[data-testid="canvas-task-body"]'), "body present near (full)");
  assert(near.host.querySelectorAll('[data-testid="canvas-task-tag-facet"]').length >= 1, "facet chips in meta band near");
  await act(async () => near.root.unmount());
});

test("TaskCard §24.4 zoom-drill: W0/W1 stay compact — body slot hidden, no click state", async () => {
  const task = { ...FIXTURE_TASK, intent: "Ship the login flow safely." };
  for (const world of ["W0", "W1"] as const) {
    const { host, root } = await render(<TaskCard task={task} style={cardStyle} world={world} />);
    assert(host.querySelector('[data-testid="canvas-task-status"]'), `${world} keeps status visible`);
    assert(!host.querySelector('[data-testid="canvas-task-body"]'), `${world} has no W2+ body testid`);
    const bodySlot = host.querySelector('[data-testid="canvas-task-body-slot"]');
    assert(bodySlot?.getAttribute("data-visible") === "false", `${world} body slot reserved but not revealed`);
    assert(!isVisible(host.querySelector('[data-testid="canvas-task-intent"]')), `${world} intent not painted yet`);
    assert(!host.querySelector('[data-testid="canvas-task-full-body"]'), `${world} no unclamped body band`);
    await act(async () => root.unmount());
  }
});

test("TaskCard §24.4 zoom-drill: W2 reveals clamped body + intent; W3 adds full body and rationale", async () => {
  const task = {
    ...FIXTURE_TASK,
    intent: "Ship the login flow safely.",
    body: "Implement the **login** endpoint with OAuth redirect.",
  };
  const w2 = await render(<TaskCard task={task} style={cardStyle} world="W2" />);
  const w2Body = w2.host.querySelector('[data-testid="canvas-task-body"]');
  assert(w2Body?.getAttribute("data-visible") === "true", "W2 body slot revealed");
  assert(isVisible(w2Body), "W2 body visible without interaction");
  assert(isVisible(w2.host.querySelector('[data-testid="canvas-task-intent"]')), "W2 intent visible");
  assert(w2.host.querySelector('[data-testid="canvas-task-body-preview"]'), "W2 uses clamped preview, not full-body testid");
  assert(!w2.host.querySelector('[data-testid="canvas-task-why"]'), "model rationale waits for W3");
  await act(async () => w2.root.unmount());

  const w3 = await render(<TaskCard task={task} style={cardStyle} world="W3" />);
  assert(isVisible(w3.host.querySelector('[data-testid="canvas-task-full-body"]')), "W3 full body visible");
  assert(w3.host.textContent?.includes("Ship the login flow safely."), "W3 intent text painted");
  assert(w3.host.textContent?.includes("Complex auth logic needs a strong model."), "W3 model rationale painted");
  await act(async () => w3.root.unmount());
});

test("TaskCard §24.4 zoom-drill: stride snake deepens by world band (no click)", async () => {
  const task = taskWithStrides();
  const modes: Record<string, string> = {};
  for (const world of ["W0", "W1", "W2", "W3"] as const) {
    const { host, root } = await render(<TaskCard task={task} style={cardStyle} world={world} />);
    const steps = host.querySelector('[data-testid="canvas-task-steps"]');
    assert(steps, `${world} renders stride lane`);
    modes[world] = steps!.getAttribute("data-step-mode") ?? "";
    assert(isVisible(steps), `${world} stride lane visible without click`);
    await act(async () => root.unmount());
  }
  assert(modes.W0 === "constellation", `W0 constellation, got ${modes.W0}`);
  assert(modes.W1 === "thread", `W1 thread, got ${modes.W1}`);
  assert(modes.W2 === "cards", `W2 cards, got ${modes.W2}`);
  assert(modes.W3 === "bloom", `W3 bloom, got ${modes.W3}`);
});

test("TaskCard primary facet tints left border accent", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} tagFacets={[...FIXTURE_TAG_FACETS]} style={cardStyle} world="W1" />);
  const card = host.querySelector('[data-testid="canvas-task-card"]') as HTMLElement | null;
  assert(card?.style.borderLeftColor === "rgba(99, 102, 241, 0.95)", `primary facet border accent, got: ${card?.getAttribute("style")}`);
  await act(async () => root.unmount());
});

test("MilestoneBubble shows kind initial + iteration at rest; tooltip is hover-only (no near-LOD wall)", async () => {
  const { host, root } = await render(
    <MilestoneBubble model={{ milestone: FIXTURE_MILESTONE, color: "rgba(99,102,241,0.9)" }} style={cardStyle} lod="near" />,
  );
  assert(host.querySelector('[data-testid="canvas-milestone-initial"]')?.textContent === "S", "kind initial glyph at rest");
  assert(host.querySelector('[data-testid="canvas-milestone-iteration"]')?.textContent === "2", "iteration badge");
  // The bug: at near LOD every bubble auto-opened its tooltip → overlapping,
  // clipped, word-glued mess. Tooltip must NOT render until hover.
  assert(!host.querySelector('[data-testid="canvas-milestone-text"]'), "no auto tooltip at near LOD");
  await act(async () => root.unmount());
});

test("MilestoneBubble W2 names the kind on the face; W3 pins the detail open (deep-read without hover)", async () => {
  const ms = { ...FIXTURE_MILESTONE, links: ["spec#auth"], text: "Refactored the **token** path." };
  const w1 = await render(<MilestoneBubble model={{ milestone: ms, color: "rgba(99,102,241,0.9)" }} style={cardStyle} world="W1" />);
  assert(!w1.host.querySelector('[data-testid="canvas-milestone-kind-face"]'), "no kind face below W2");
  assert(!w1.host.querySelector('[data-testid="canvas-milestone-text"]'), "no pinned panel below W3");
  await act(async () => w1.root.unmount());

  const w3 = await render(<MilestoneBubble model={{ milestone: ms, color: "rgba(99,102,241,0.9)" }} style={cardStyle} world="W3" />);
  assert(w3.host.querySelector('[data-testid="canvas-milestone-kind-face"]')?.textContent === "status update", "kind named on face at W2+");
  assert(w3.host.querySelector('[data-testid="canvas-milestone-text"]'), "detail panel pinned open at W3 (no hover)");
  assert(w3.host.textContent?.includes("token"), "full text readable at W3");
  assert(w3.host.querySelector('[data-testid="canvas-milestone-links"]')?.textContent?.includes("spec#auth"), "links shown at W3");
  await act(async () => w3.root.unmount());
});

test("StrideSnake names each working point by kind at W1/W2 (distinguishable, kind-colored)", async () => {
  const ms = [
    { ...FIXTURE_MILESTONE, id: "a", kind: "testing", text: "ran the suite" },
    { ...FIXTURE_MILESTONE, id: "b", kind: "decision", text: "chose snake layout" },
  ];
  const w1 = await render(<StrideSnake milestones={ms} world="W1" width={280} />);
  const kinds1 = [...w1.host.querySelectorAll('[data-testid="canvas-stride-kind"]')].map((e) => e.textContent);
  assert(kinds1.includes("testing") && kinds1.includes("decision"), "W1 labels name the kind");
  await act(async () => w1.root.unmount());

  const w2 = await render(<StrideSnake milestones={ms} world="W2" width={280} />);
  assert([...w2.host.querySelectorAll('[data-testid="canvas-stride-kind"]')].some((e) => e.textContent === "testing"), "W2 callout names the kind");
  assert(w2.host.querySelector('[data-testid="canvas-task-step-dot"][data-milestone-kind="testing"]'), "bead carries kind for color/identity");
  await act(async () => w2.root.unmount());
});

test("StrideSnake W3 bloom renders attached discussion doc collapsed by default", async () => {
  const milestone = {
    ...FIXTURE_MILESTONE,
    text: "STATUS: done\n\ndoc: notes/design.md\n\n# Design notes\n\nExpand me.",
  };
  const { host, root } = await render(
    <StrideSnake milestones={[milestone]} world="W3" width={280} />,
  );
  assert(host.querySelector('[data-testid="attached-discussion-doc"]'), "discussion doc chrome");
  assert(host.textContent?.includes("STATUS: done"), "summary visible");
  assert(!host.textContent?.includes("Expand me."), "doc body hidden until expand");
  assert(host.querySelector('[data-testid="attached-discussion-toggle"]'), "expand toggle");
  await act(async () => root.unmount());
});

test("§34 single-camera transform: no counter-scaled crisp re-raster layer at any zoom", async () => {
  // Crisp wrapper was removed (single camera transform) — no .ct-canvas-crisp at 1×.
  const plain = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />);
  assert(!plain.host.querySelector(".ct-canvas-crisp"), "no crisp layer at 1×");
  assert(plain.host.querySelector('[data-testid="canvas-plan-node"]'), "card still renders at 1×");
  await act(async () => plain.root.unmount());

  // Deep zoom no longer re-rasterizes: still no wrapper, content reaches the DOM directly.
  const deep = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" crispScale={4} />);
  assert(!deep.host.querySelector(".ct-canvas-crisp"), "no crisp layer at 4× (single camera transform)");
  assert(deep.host.querySelector('[data-testid="canvas-plan-node"]'), "card renders at 4× without re-raster wrapper");
  assert(deep.host.textContent?.includes("Auth rollout"), "title rendered directly, not through a crisp layer");
  await act(async () => deep.root.unmount());
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();

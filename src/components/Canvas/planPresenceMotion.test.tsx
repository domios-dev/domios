import { act, type ReactElement } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { PlanSidePanelLayer } from "./PlanSidePanelHost";
import { planSidePanelStore } from "./planSidePanelStore";
import { PlanViewModeSwitch, type TemporaryModeTab } from "./PlanViewModeSwitch";
import { TaskStatus, type Plan, type Task } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FIRST: Task = { ...FIXTURE_TASK, id: "task_motion1", key: "CT-11", name: "Slide in", status: TaskStatus.Pending };
const SECOND: Task = { ...FIXTURE_TASK, id: "task_motion2", key: "CT-12", name: "Replace", status: TaskStatus.Pending };
const PLAN: Plan = { ...FIXTURE_PLAN, id: "pl_motion", status: "active", tasks: [FIRST, SECOND] };

const TEMPORARY_TAB: TemporaryModeTab = {
  value: "task",
  label: "Task CT-11",
  closeLabel: "Close Task CT-11",
  tabId: "segment-tab",
  panelId: "segment-panel",
  onClose: () => {},
};

const mounted: Array<{ root: Root; host: HTMLElement }> = [];
const restorers: Array<() => void> = [];

async function mount(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(ui));
  return { host, rerender: (next: ReactElement) => act(async () => root.render(next)) };
}

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  for (const restore of restorers.splice(0)) restore();
  act(() => {
    planSidePanelStore.close();
  });
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Stands in for the stylesheet: the animation-name each presence element carries in each state. */
const PANEL_ANIMATIONS: Record<string, string> = { entering: "ct-plan-side-panel-in", closing: "ct-plan-side-panel-out" };
const SEGMENT_ANIMATIONS: Record<string, string> = {
  entering: "ct-segmented-donut-grow, ct-presence-fade-in",
  closing: "ct-plan-view-segment-shrink",
};

function styledAnimationName(element: Element): string | null {
  const state = element.getAttribute("data-state") ?? "";
  if (element.classList.contains("plan-side-panel")) return PANEL_ANIMATIONS[state] ?? "none";
  if (element.classList.contains("plan-view-modes__group--temporary")) return SEGMENT_ANIMATIONS[state] ?? "none";
  return null;
}

function stubStylesheetAnimations() {
  const original = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation((element: Element, pseudo?: string | null) => {
    const animationName = styledAnimationName(element);
    return animationName === null ? original(element, pseudo) : ({ animationName } as CSSStyleDeclaration);
  });
}

type RunningAnimation = Pick<CSSAnimation, "animationName" | "playState">;

/** Stands in for the browser's own animation list, which the stylesheet name alone cannot tell apart from a hidden element. */
function stubElementAnimations(read: () => RunningAnimation[]) {
  const prototype = HTMLElement.prototype as { getAnimations?: unknown };
  const original = Object.getOwnPropertyDescriptor(prototype, "getAnimations");
  Object.defineProperty(prototype, "getAnimations", { configurable: true, value: () => read() });
  restorers.push(() => {
    if (original) Object.defineProperty(prototype, "getAnimations", original);
    else delete prototype.getAnimations;
  });
}

function stubReducedMotion() {
  vi.spyOn(window, "matchMedia").mockImplementation((query: string) => ({
    matches: query.includes("prefers-reduced-motion: reduce"),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  }) as MediaQueryList);
}

async function fireAnimation(element: Element, animationName: string, type: "animationend" | "animationcancel" = "animationend") {
  const event = new Event(type, { bubbles: true });
  Object.defineProperty(event, "animationName", { value: animationName });
  await act(async () => {
    element.dispatchEvent(event);
  });
}

function sidePanel(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-testid="plan-side-panel"]');
}

async function openTask(task: Task) {
  await act(async () => {
    planSidePanelStore.open({ planId: PLAN.id, taskId: task.id });
  });
}

async function mountPanelWithOpener() {
  const { host } = await mount(
    <>
      <button type="button" data-testid="opener">open</button>
      <PlanSidePanelLayer plans={[PLAN]} tabs={{}} onOpenPlanTarget={() => {}} />
    </>,
  );
  const opener = host.querySelector<HTMLButtonElement>('[data-testid="opener"]') as HTMLButtonElement;
  opener.focus();
  return opener;
}

describe("side panel presence", () => {
  it("keeps the closing panel mounted until its own exit animation ends, then returns focus to the opener", async () => {
    stubStylesheetAnimations();
    const opener = await mountPanelWithOpener();
    await openTask(FIRST);

    const panel = sidePanel() as HTMLElement;
    expect(panel.getAttribute("data-state")).toBe("entering");
    expect(document.activeElement).toBe(panel);
    await fireAnimation(panel, "ct-plan-side-panel-in");
    expect(panel.getAttribute("data-state")).toBe("open");

    await act(async () => {
      panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(sidePanel()).toBe(panel);
    expect(panel.getAttribute("data-state")).toBe("closing");
    expect(panel.hasAttribute("inert")).toBe(true);
    expect(document.activeElement).not.toBe(opener);

    await fireAnimation(panel, "ct-toast-out");
    expect(sidePanel()?.getAttribute("data-state")).toBe("closing");
    const body = panel.querySelector('[data-testid="plan-side-panel-body"]') as HTMLElement;
    await fireAnimation(body, "ct-plan-side-panel-out");
    expect(sidePanel()?.getAttribute("data-state")).toBe("closing");

    await fireAnimation(panel, "ct-plan-side-panel-out");
    expect(sidePanel()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("an interrupted exit animation still unmounts through animationcancel", async () => {
    stubStylesheetAnimations();
    await mountPanelWithOpener();
    await openTask(FIRST);
    const panel = sidePanel() as HTMLElement;
    await act(async () => (panel.querySelector('[data-testid="plan-side-panel-close"]') as HTMLButtonElement).click());
    expect(panel.getAttribute("data-state")).toBe("closing");

    await fireAnimation(panel, "ct-plan-side-panel-out", "animationcancel");
    expect(sidePanel()).toBeNull();
  });

  it("under reduced motion opens and closes without waiting for any animation event", async () => {
    stubStylesheetAnimations();
    stubReducedMotion();
    const opener = await mountPanelWithOpener();
    await openTask(FIRST);
    const panel = sidePanel() as HTMLElement;
    expect(panel.getAttribute("data-state")).toBe("open");

    await act(async () => (panel.querySelector('[data-testid="plan-side-panel-close"]') as HTMLButtonElement).click());
    expect(sidePanel()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("replacing the content keeps the same open panel, does not replay its entry, and fades the new body in", async () => {
    stubStylesheetAnimations();
    await mountPanelWithOpener();
    await openTask(FIRST);
    const panel = sidePanel() as HTMLElement;
    expect(panel.querySelector('[data-testid="plan-side-panel-body"]')?.hasAttribute("data-replaced")).toBe(false);
    await fireAnimation(panel, "ct-plan-side-panel-in");

    await openTask(SECOND);
    expect(sidePanel()).toBe(panel);
    expect(panel.getAttribute("data-state")).toBe("open");
    expect(panel.getAttribute("data-task-id")).toBe(SECOND.id);
    expect(panel.querySelector('[data-testid="plan-side-panel-body"]')?.hasAttribute("data-replaced")).toBe(true);
  });

  it("reopening during the exit enters again on the same panel", async () => {
    stubStylesheetAnimations();
    await mountPanelWithOpener();
    await openTask(FIRST);
    const panel = sidePanel() as HTMLElement;
    await fireAnimation(panel, "ct-plan-side-panel-in");
    await act(async () => {
      planSidePanelStore.close();
    });
    expect(panel.getAttribute("data-state")).toBe("closing");

    await openTask(SECOND);
    expect(sidePanel()).toBe(panel);
    expect(panel.getAttribute("data-state")).toBe("entering");
    await fireAnimation(panel, "ct-plan-side-panel-out", "animationcancel");
    expect(sidePanel()?.getAttribute("data-task-id")).toBe(SECOND.id);
  });
});

describe("temporary task segment presence", () => {
  function temporaryGroup(host: HTMLElement): HTMLElement | null {
    return host.querySelector<HTMLElement>('[data-testid="plan-view-temporary-group"]');
  }

  it("grows in and shrinks out on its own animations while the selection moves back to Tasks", async () => {
    stubStylesheetAnimations();
    const onChange = vi.fn();
    const { host, rerender } = await mount(<PlanViewModeSwitch value="task" onChange={onChange} temporaryTab={TEMPORARY_TAB} />);

    const group = temporaryGroup(host) as HTMLElement;
    expect(group.getAttribute("data-state")).toBe("entering");
    await fireAnimation(group, "ct-presence-fade-in");
    expect(group.getAttribute("data-state")).toBe("entering");
    await fireAnimation(group, "ct-segmented-donut-grow");
    expect(group.getAttribute("data-state")).toBe("open");

    await rerender(<PlanViewModeSwitch value="tasks" onChange={onChange} temporaryTab={null} />);
    expect(temporaryGroup(host)).toBe(group);
    expect(group.getAttribute("data-state")).toBe("closing");
    expect(group.hasAttribute("inert")).toBe(true);
    expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')?.getAttribute("aria-selected")).toBe("false");
    expect(host.querySelector('[role="tab"][data-value="tasks"]')?.getAttribute("aria-selected")).toBe("true");

    await fireAnimation(group, "ct-segmented-donut-grow");
    expect(temporaryGroup(host)?.getAttribute("data-state")).toBe("closing");
    await fireAnimation(group, "ct-plan-view-segment-shrink");
    expect(temporaryGroup(host)).toBeNull();
  });

  it("with two animations on the group, opens when the last one ends even if the grow keyframe's own event never arrived", async () => {
    stubStylesheetAnimations();
    let animations: RunningAnimation[] = [
      { animationName: "ct-segmented-donut-grow", playState: "running" },
      { animationName: "ct-presence-fade-in", playState: "running" },
    ];
    stubElementAnimations(() => animations);
    const { host } = await mount(<PlanViewModeSwitch value="task" onChange={() => {}} temporaryTab={TEMPORARY_TAB} />);
    const group = temporaryGroup(host) as HTMLElement;
    expect(group.getAttribute("data-state")).toBe("entering");

    animations = [animations[0]!, { animationName: "ct-presence-fade-in", playState: "finished" }];
    await fireAnimation(group, "ct-presence-fade-in");
    expect(group.getAttribute("data-state")).toBe("entering");

    animations = animations.map((animation) => ({ ...animation, playState: "finished" }));
    await fireAnimation(group.querySelector('[data-testid="plan-view-temporary-tab"]') as HTMLElement, "ct-presence-fade-in");
    expect(group.getAttribute("data-state")).toBe("entering");
    await fireAnimation(group, "ct-presence-fade-in");
    expect(group.getAttribute("data-state")).toBe("open");
  });

  it("a group the browser runs no animation on (not rendered) opens and leaves without waiting", async () => {
    stubStylesheetAnimations();
    stubElementAnimations(() => []);
    const { host, rerender } = await mount(<PlanViewModeSwitch value="task" onChange={() => {}} temporaryTab={TEMPORARY_TAB} />);
    expect(temporaryGroup(host)?.getAttribute("data-state")).toBe("open");

    await rerender(<PlanViewModeSwitch value="tasks" onChange={() => {}} temporaryTab={null} />);
    expect(temporaryGroup(host)).toBeNull();
  });

  it("under reduced motion the segment appears and leaves without waiting for an event", async () => {
    stubStylesheetAnimations();
    stubReducedMotion();
    const { host, rerender } = await mount(<PlanViewModeSwitch value="task" onChange={() => {}} temporaryTab={TEMPORARY_TAB} />);
    expect(temporaryGroup(host)?.getAttribute("data-state")).toBe("open");

    await rerender(<PlanViewModeSwitch value="tasks" onChange={() => {}} temporaryTab={null} />);
    expect(temporaryGroup(host)).toBeNull();
  });

  it("the thumb lands without motion first and glides for every later selection, however it changed", async () => {
    const { host, rerender } = await mount(<PlanViewModeSwitch value="tasks" onChange={() => {}} />);
    const thumb = host.querySelector('[data-testid="plan-view-mode-thumb"]') as HTMLElement;
    expect(thumb.classList.contains("plan-view-modes__thumb--glide")).toBe(false);

    await rerender(<PlanViewModeSwitch value="task" onChange={() => {}} temporaryTab={TEMPORARY_TAB} />);
    expect(thumb.classList.contains("plan-view-modes__thumb--glide")).toBe(true);
  });
});

const readSource = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8").replaceAll("\r\n", "\n");

function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`\n${selector} {`);
  expect(start, `${selector} rule`).toBeGreaterThanOrEqual(0);
  const open = css.indexOf("{", start);
  return css.slice(open + 1, css.indexOf("}", open));
}

function keyframesBody(css: string, name: string): string {
  const start = css.indexOf(`@keyframes ${name}`);
  expect(start, `@keyframes ${name}`).toBeGreaterThanOrEqual(0);
  const open = css.indexOf("{", start);
  let depth = 0;
  for (let index = open; index < css.length; index += 1) {
    if (css[index] === "{") depth += 1;
    if (css[index] === "}") depth -= 1;
    if (depth === 0) return css.slice(open + 1, index);
  }
  throw new Error(`unterminated @keyframes ${name}`);
}

describe("presence motion stylesheet", () => {
  const css = readSource("../../styles/globals.css");
  const MOTION_RULES = [
    '.plan-side-panel[data-state="entering"]',
    '.plan-side-panel[data-state="closing"]',
    ".plan-side-panel__body[data-replaced]",
    '.plan-view-modes__group--temporary[data-state="entering"]',
    '.plan-view-modes__group--temporary[data-state="closing"]',
  ];

  it("animates only transform and opacity with motion tokens", () => {
    for (const selector of MOTION_RULES) {
      const body = ruleBody(css, selector);
      expect(body, selector).not.toMatch(/transition/);
      const value = /animation:\s*([^;]+);/.exec(body)?.[1];
      expect(value, `${selector} animation`).toBeTruthy();
      for (const layer of (value as string).split(",")) {
        const [name] = layer.trim().split(/\s+/);
        expect(layer, `${selector} ${name}`).toMatch(/var\(--ct-duration-[\w-]+\)/);
        expect(layer, `${selector} ${name}`).not.toMatch(/\b\d+(\.\d+)?m?s\b/);
        const properties = new Set(Array.from(keyframesBody(css, name).matchAll(/([a-z-]+)\s*:/g), (match) => match[1]));
        expect([...properties].every((property) => property === "opacity" || property === "transform"), `${name} properties ${[...properties]}`).toBe(true);
      }
    }
    expect(css).not.toMatch(/\.(plan-side-panel|plan-view-modes)[^{]*\{[^}]*transition(-property)?:\s*all\b/);
  });

  it("drops every presence animation and the thumb glide under reduced motion", () => {
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{\n {2}\.plan-side-panel\[data-state\],\n {2}\.plan-side-panel__body\[data-replaced\],\n {2}\.plan-view-modes__group--temporary\[data-state\] \{\n {4}animation: none;\n {2}\}\n {2}\.plan-view-modes__thumb--glide \{\n {4}transition: none;/,
    );
  });

  it("waits on events, never timers, and never transitions everything", () => {
    for (const path of ["./planView/PlanSidePanel.tsx", "./PlanSidePanelHost.tsx", "./PlanViewModeSwitch.tsx", "./usePresence.ts"]) {
      const source = readSource(path);
      expect(source, path).not.toMatch(/setTimeout|setInterval|requestAnimationFrame/);
      expect(source, path).not.toMatch(/transition-all/);
    }
  });
});

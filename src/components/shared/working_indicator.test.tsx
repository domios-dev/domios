import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ActivityDots } from "./ActivityDots";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const repoRoot = process.cwd();
const sourceFiles = [
  "src/components/Terminal/AgentStateIndicator.tsx",
  "src/components/shared/BusyState.tsx",
  "src/components/Sidebar/TabItem.tsx",
  "src/components/Sidebar/TeamUnit.tsx",
  "src/components/Sidebar/WorkspaceCardGrid.tsx",
  "src/components/Canvas/PlanButton.tsx",
  "src/components/Canvas/planView/PlanCompanionBubble.tsx",
];

let host: HTMLElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function render(active: boolean) {
  await act(async () => root.render(<ActivityDots active={active} data-testid="working-indicator" />));
  return host.querySelector<HTMLElement>('[data-testid="working-indicator"]')!;
}

async function finishCooldown(dots: HTMLElement) {
  const event = new Event("animationend", { bubbles: true });
  Object.defineProperty(event, "animationName", { value: "ct-dots-cooldown" });
  await act(async () => dots.dispatchEvent(event));
}

describe("working_indicator", () => {
  it("maps active, idle cooling, still, and reactivation transitions", async () => {
    const dots = await render(true);
    expect(dots.dataset.activityState).toBe("active");

    const cooling = await render(false);
    expect(cooling).toBe(dots);
    expect(cooling.dataset.active).toBe("false");
    expect(cooling.dataset.activityState).toBe("cooling");

    await finishCooldown(cooling);
    expect(cooling.dataset.activityState).toBe("still");

    const reactivated = await render(true);
    expect(reactivated).toBe(dots);
    expect(reactivated.dataset.activityState).toBe("reactivated");

    const coolingAgain = await render(false);
    expect(coolingAgain.dataset.activityState).toBe("cooling");
  });

  it("settles without JavaScript timers", () => {
    const component = readFileSync(resolve(repoRoot, "src/components/shared/ActivityDots.tsx"), "utf8");
    expect(component).not.toMatch(/setTimeout|setInterval|requestAnimationFrame/);
  });

  it("preserves the dot count across activity states", async () => {
    const dots = await render(true);
    const activeCount = dots.children.length;
    const inactive = await render(false);
    expect(inactive.children.length).toBe(activeCount);
  });

  it("uses ActivityDots as the owner for every shipped working-dot surface", () => {
    for (const relativePath of sourceFiles) {
      const source = readFileSync(resolve(repoRoot, relativePath), "utf8");
      expect(source, relativePath).toMatch(/ActivityDots/);
    }
    const tabItem = readFileSync(resolve(repoRoot, "src/components/Sidebar/TabItem.tsx"), "utf8");
    expect(tabItem).toMatch(/tabActivity\(tab, status\)/);
    expect(tabItem).toMatch(/<TabWorkingActivity activity=\{tabState\} \/>/);
    const agentState = readFileSync(
      resolve(repoRoot, "src/components/Terminal/AgentStateIndicator.tsx"),
      "utf8",
    );
    expect(agentState).toMatch(/active=\{activity.active\}/);
  });
});

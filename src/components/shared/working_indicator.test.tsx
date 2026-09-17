import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ActivityDots } from "./ActivityDots";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const repoRoot = process.cwd();
const css = readFileSync(resolve(repoRoot, "src/styles/agent-state.css"), "utf8");
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
    expect(dots.classList.contains("activity-dots--active")).toBe(true);

    const cooling = await render(false);
    expect(cooling).toBe(dots);
    expect(cooling.dataset.active).toBe("false");
    expect(cooling.dataset.activityState).toBe("cooling");
    expect(cooling.classList.contains("activity-dots--cooling")).toBe(true);

    await finishCooldown(cooling);
    expect(cooling.dataset.activityState).toBe("still");
    expect(cooling.classList.contains("activity-dots--still")).toBe(true);

    const reactivated = await render(true);
    expect(reactivated).toBe(dots);
    expect(reactivated.dataset.activityState).toBe("reactivated");
    expect(reactivated.classList.contains("activity-dots--reactivated")).toBe(true);

    const coolingAgain = await render(false);
    expect(coolingAgain.dataset.activityState).toBe("cooling");
  });

  it("uses one shared cooldown clock and settles without JavaScript timers", () => {
    expect(css).toMatch(/--activity-dot-cooldown-duration:\s*30s;/);
    expect(css).toMatch(
      /\.activity-dots--cooling i\s*\{[^}]*animation:\s*ct-dots-cooldown var\(--activity-dot-cooldown-duration\)[^;]*forwards;/s,
    );
    expect(css).toMatch(
      /\.activity-dots--still i\s*\{[^}]*animation:\s*none;[^}]*opacity:\s*0;[^}]*transform:\s*scale\(0\.9\);/s,
    );
    expect(css).toMatch(/@keyframes ct-dots-cooldown[\s\S]*100%\s*\{[^}]*transform:\s*scale\(0\.9\);/);
    const component = readFileSync(resolve(repoRoot, "src/components/shared/ActivityDots.tsx"), "utf8");
    expect(component).not.toMatch(/setTimeout|setInterval|requestAnimationFrame/);
  });

  it("keeps reduced-motion static and preserves the dot slot geometry", async () => {
    const dots = await render(true);
    const activeCount = dots.children.length;
    const inactive = await render(false);
    expect(inactive.children.length).toBe(activeCount);
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.activity-dots i\s*\{[^}]*animation:\s*none !important;[^}]*opacity:\s*0\.85;[^}]*transform:\s*scale\(1\);/s,
    );
    expect(css).toMatch(/\.ct-phase-dots\s*\{[^}]*height:\s*calc\([^}]*\);[^}]*flex:\s*0 0 auto;/s);
    for (const size of ["micro", "small", "medium"]) {
      expect(css).toMatch(new RegExp(`\\.activity-dots--${size}\\s*\\{[^}]*--activity-dot-size:`));
    }
  });

  it("uses ActivityDots as the owner for every shipped working-dot surface", () => {
    for (const relativePath of sourceFiles) {
      const source = readFileSync(resolve(repoRoot, relativePath), "utf8");
      expect(source, relativePath).toMatch(/ActivityDots/);
    }
    const tabItem = readFileSync(resolve(repoRoot, "src/components/Sidebar/TabItem.tsx"), "utf8");
    expect(tabItem).toMatch(/status === "Working" \|\| status === "Idle"/);
    expect(tabItem).toMatch(/<TabWorkingActivity active=\{status === "Working"\} \/>/);
    const agentState = readFileSync(
      resolve(repoRoot, "src/components/Terminal/AgentStateIndicator.tsx"),
      "utf8",
    );
    expect(agentState).toMatch(/active=\{isWorking\}/);
  });
});

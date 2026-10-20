import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ActivityDots } from "./ActivityDots";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

  it("preserves the dot count across activity states", async () => {
    const dots = await render(true);
    const activeCount = dots.children.length;
    const inactive = await render(false);
    expect(inactive.children.length).toBe(activeCount);
  });

});

import assert from "node:assert/strict";
import { ShellPurposeIcon } from "./ShellStatus";
import { act, createRoot, createSuite, setupDom } from "../Sidebar/SidebarWorkspaceTabs.testkit";

setupDom();
const { test, run } = createSuite("Hammer strike lifecycle");

for (const purpose of ["build", "qa"] as const) {
  test(`only running ${purpose} tabs animate, with live reduced-motion and cleanup`, async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const originalMatchMedia = window.matchMedia;
    const originalAnimate = SVGElement.prototype.animate;
    let starts = 0;
    let cancels = 0;
    window.matchMedia = () => motion;
    SVGElement.prototype.animate = () => {
      starts += 1;
      return { cancel: () => { cancels += 1; } } as Animation;
    };
    const reduce = async (matches: boolean) => {
      Object.defineProperty(motion, "matches", { value: matches, configurable: true });
      await act(async () => motion.dispatchEvent(new Event("change")));
    };
    try {
      await reduce(false);
      await act(async () => root.render(<ShellPurposeIcon purpose={purpose} />));
      assert.ok(host.querySelector(`[role="img"][aria-label="${purpose} shell"]`));
      assert.equal(starts, 0);
      await act(async () => root.render(<ShellPurposeIcon purpose="dev" running />));
      assert.equal(starts, 0);
      await act(async () => root.render(<ShellPurposeIcon purpose={purpose} running />));
      assert.equal(starts, 1);
      await reduce(true);
      assert.equal(cancels, 1);
      assert.equal(starts, 1);
      await reduce(false);
      assert.equal(starts, 2);
      await act(async () => root.render(<ShellPurposeIcon purpose={purpose} />));
      assert.equal(cancels, 2);
      await reduce(true);
      await act(async () => root.render(<ShellPurposeIcon purpose={purpose} running />));
      assert.equal(starts, 2);
      await reduce(false);
      assert.equal(starts, 3);
      await act(async () => root.unmount());
      assert.equal(cancels, 3);
      await reduce(true);
      await reduce(false);
      assert.equal(starts, 3);
    } finally {
      await act(async () => root.unmount());
      window.matchMedia = originalMatchMedia;
      SVGElement.prototype.animate = originalAnimate;
      host.remove();
    }
  });

}

void run();

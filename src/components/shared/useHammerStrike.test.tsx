import assert from "node:assert/strict";
import { ShellPurposeIcon } from "./ShellStatus";
import { act, createRoot, createSuite, setupDom } from "../Sidebar/SidebarWorkspaceTabs.testkit";

setupDom();
const { test, run } = createSuite("Hammer strike lifecycle");

test("only running builds animate, with live reduced-motion and cleanup", async () => {
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
    await act(async () => root.render(<ShellPurposeIcon purpose="build" />));
    assert.ok(host.querySelector('[role="img"][aria-label="build shell"]'));
    assert.equal(starts, 0);
    await act(async () => root.render(<ShellPurposeIcon purpose="dev" running />));
    assert.equal(starts, 0);
    await act(async () => root.render(<ShellPurposeIcon purpose="build" running />));
    assert.equal(starts, 1);
    await reduce(true);
    assert.equal(cancels, 1);
    assert.equal(starts, 1);
    await reduce(false);
    assert.equal(starts, 2);
    await act(async () => root.render(<ShellPurposeIcon purpose="build" />));
    assert.equal(cancels, 2);
    await reduce(true);
    await act(async () => root.render(<ShellPurposeIcon purpose="build" running />));
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

void run();

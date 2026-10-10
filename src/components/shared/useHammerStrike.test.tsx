import assert from "node:assert/strict";
import { ShellPurposeIcon } from "./ShellStatus";
import { act, createRoot, createSuite, setupDom } from "../Sidebar/SidebarWorkspaceTabs.testkit";

setupDom();
const { test, run } = createSuite("Hammer strike lifecycle");

test("running builds synchronize the swing and three impact sparks, honoring reduced motion and cleanup", async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const originalMatchMedia = window.matchMedia;
  const originalAnimate = SVGElement.prototype.animate;
  const animations: { frames: Keyframe[]; timing: KeyframeAnimationOptions }[] = [];
  let cancels = 0;
  let unmounted = false;
  window.matchMedia = () => motion;
  SVGElement.prototype.animate = (frames, timing) => {
    animations.push({ frames: frames as Keyframe[], timing: timing as KeyframeAnimationOptions });
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
    assert.equal(animations.length, 0);
    await act(async () => root.render(<ShellPurposeIcon purpose="dev" running />));
    assert.equal(animations.length, 0);
    await act(async () => root.render(<ShellPurposeIcon purpose="build" running />));
    assert.equal(animations.length, 4);
    const [swing, ...sparks] = animations;
    const impact = swing.frames.find((frame) => frame.transform === "rotate(30deg)");
    assert.ok(impact);
    for (const spark of sparks) {
      assert.deepEqual(spark.timing, swing.timing);
      const visible = spark.frames.findIndex((frame) => Number(frame.opacity) > 0);
      assert.ok(visible > 0);
      assert.equal(spark.frames[visible].offset, impact.offset);
      const fadeMs = (Number(spark.frames[visible + 1].offset) - Number(impact.offset)) * Number(spark.timing.duration);
      assert.ok(fadeMs > 0 && fadeMs <= 250);
      assert.equal(spark.frames[visible + 1].opacity, 0);
      assert.equal(spark.frames[spark.frames.length - 1].opacity, 0);
    }
    await reduce(true);
    assert.equal(cancels, 4);
    assert.equal(animations.length, 4);
    await reduce(false);
    assert.equal(animations.length, 8);
    await act(async () => root.render(<ShellPurposeIcon purpose="build" />));
    assert.equal(cancels, 8);
    await reduce(true);
    await act(async () => root.render(<ShellPurposeIcon purpose="build" running />));
    assert.equal(animations.length, 8);
    await reduce(false);
    assert.equal(animations.length, 12);
    await act(async () => root.unmount());
    unmounted = true;
    assert.equal(cancels, 12);
    await reduce(true);
    await reduce(false);
    assert.equal(animations.length, 12);
  } finally {
    if (!unmounted) await act(async () => root.unmount());
    window.matchMedia = originalMatchMedia;
    SVGElement.prototype.animate = originalAnimate;
    host.remove();
  }
});

void run();

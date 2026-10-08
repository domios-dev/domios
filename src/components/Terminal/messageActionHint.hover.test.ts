import { afterEach, expect, it, vi } from "vitest";
import { createStillHoverHint } from "./messageActionHint";

afterEach(() => vi.useRealTimers());

function harness() {
  vi.useFakeTimers();
  let motion = true;
  const show = vi.fn();
  const claim = vi.fn(() => true);
  const hover = createStillHoverHint({
    schedule: (fn, delay) => setTimeout(fn, delay),
    cancel: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
    allowed: () => motion,
    claim,
    show,
  });
  return { hover, show, claim, disable: () => { motion = false; } };
}

it("waits for a still pointer and hints only once per hover", () => {
  const { hover, show } = harness();
  hover.enter();
  vi.advanceTimersByTime(599);
  expect(show).not.toHaveBeenCalled();
  hover.move();
  vi.advanceTimersByTime(599);
  expect(show).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1);
  expect(show).toHaveBeenCalledTimes(1);
  hover.move();
  vi.advanceTimersByTime(2000);
  expect(show).toHaveBeenCalledTimes(1);
  hover.leave();
  hover.enter();
  vi.advanceTimersByTime(600);
  expect(show).toHaveBeenCalledTimes(2);
});

it("cancels on leave or menu open", () => {
  const { hover, show } = harness();
  hover.enter();
  hover.leave();
  vi.advanceTimersByTime(600);
  hover.enter();
  hover.stop();
  vi.advanceTimersByTime(600);
  expect(show).not.toHaveBeenCalled();
});

it("does not claim a hint with motion disabled, including during the dwell", () => {
  const { hover, show, claim, disable } = harness();
  hover.enter();
  disable();
  vi.advanceTimersByTime(600);
  hover.leave();
  hover.enter();
  vi.advanceTimersByTime(600);
  expect(show).not.toHaveBeenCalled();
  expect(claim).not.toHaveBeenCalled();
});

it("does not show a hint when the shared budget refuses it", () => {
  const { hover, show, claim } = harness();
  claim.mockReturnValue(false);
  hover.enter();
  vi.advanceTimersByTime(600);
  hover.move();
  vi.advanceTimersByTime(600);
  expect(show).not.toHaveBeenCalled();
  expect(claim).toHaveBeenCalledTimes(1);
});

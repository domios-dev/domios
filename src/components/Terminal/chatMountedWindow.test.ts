import { describe, expect, it } from "vitest";
import {
  CHAT_MOUNT_MAX,
  CHAT_MOUNT_TARGET,
  chatRowOffsets,
  mountedChatWindow,
  mountedChatWindowCoversViewport,
  mountedChatWindowCoversVisibleViewport,
  nextChatScrollVelocity,
} from "./chatMountedWindow";
import { IndexedChatRowGeometry } from "./chatRowGeometry";

describe("measured chat mounting", () => {
  it("mounts one viewport of overscan and preserves the measured extent", () => {
    const window = mountedChatWindow({ heights: Array(220).fill(100), scrollTop: 8000, viewportHeight: 500 });
    expect(window).toEqual({ start: 75, end: 90, top: 7500, bottom: 13000, total: 22000 });
  });
  it("bounds dense rows to sixty while including the viewport", () => {
    const window = mountedChatWindow({ heights: Array(220).fill(10), scrollTop: 800, viewportHeight: 500 });
    expect(window.end - window.start).toBe(60);
    expect(window.start).toBeLessThanOrEqual(80);
    expect(window.end).toBeGreaterThanOrEqual(130);
  });
  it("caps overscan at the normal target", () => {
    const window = mountedChatWindow({ heights: Array(220).fill(20), scrollTop: 800, viewportHeight: 500 });
    expect(window.end - window.start).toBe(40);
    expect(window.start).toBeLessThanOrEqual(40);
    expect(window.end).toBeGreaterThanOrEqual(65);
  });
  it("includes the live tail and mounts an explicit older target", () => {
    const heights = Array(220).fill(120);
    expect(mountedChatWindow({ heights, scrollTop: 0, viewportHeight: 500, pinned: true }).end).toBe(220);
    const target = mountedChatWindow({ heights, scrollTop: 20000, viewportHeight: 500, targetIndex: 30 });
    expect(target.start).toBeLessThanOrEqual(30);
    expect(target.end).toBeGreaterThan(30);
  });
  it("drops every rich row for an inactive warm chat", () => {
    expect(mountedChatWindow({ heights: [100, 300, 50], scrollTop: 100, viewportHeight: 200, active: false }))
      .toEqual({ start: 0, end: 0, top: 0, bottom: 450, total: 450 });
  });
  it("uses complete row heights including rails, keeps empty rows at zero and estimates invalid measurements", () => {
    expect(chatRowOffsets([100, 450, 200, 0, -1, NaN])).toEqual([0, 100, 550, 750, 750, 870, 990]);
    const window = mountedChatWindow({ heights: [100, 450, 200, 80], scrollTop: 400, viewportHeight: 100 });
    expect(window.start).toBe(1);
    expect(window.top).toBe(100);
    expect(window.bottom).toBe(80);
  });
  it("recognises when the current mounted slice still covers the scrolled viewport with margin", () => {
    const geometry = new IndexedChatRowGeometry();
    for (let index = 0; index < 300; index++) geometry.append(100);
    const window = mountedChatWindow({ heights: Array(300).fill(100), scrollTop: 15000, viewportHeight: 500 });
    expect(mountedChatWindowCoversViewport(window, geometry, 15000, 500)).toBe(true);
    expect(mountedChatWindowCoversViewport(window, geometry, 14800, 500)).toBe(true);
    expect(mountedChatWindowCoversViewport(window, geometry, 14400, 500)).toBe(false);
    expect(mountedChatWindowCoversViewport(window, geometry, 16000, 500)).toBe(false);
  });
  it("expands leading overscan and coverage margin at high scroll velocity while staying bounded", () => {
    const heights = Array(300).fill(100);
    const geometry = new IndexedChatRowGeometry();
    for (const h of heights) geometry.append(h);

    const stationary = mountedChatWindow({ heights, scrollTop: 15000, viewportHeight: 600, velocity: 0 });
    const fastUp = mountedChatWindow({ heights, scrollTop: 15000, viewportHeight: 600, velocity: -500 });
    const fastDown = mountedChatWindow({ heights, scrollTop: 15000, viewportHeight: 600, velocity: 500 });

    // Upward 500 px/frame flick adds 2x velocity (1000 px) leading overscan above viewport (1600 px total above).
    expect(fastUp.top).toBeLessThanOrEqual(15000 - 1600);
    expect(fastUp.top).toBeLessThan(stationary.top);
    expect(fastUp.end).toBe(stationary.end);

    // Downward 500 px/frame flick expands leading overscan below viewport.
    expect(fastDown.total - fastDown.bottom).toBeGreaterThanOrEqual(15000 + 600 + 1600);
    expect(fastDown.start).toBe(stationary.start);

    // Velocity-scaled coverage margin triggers window shift well before a 500 px/frame step outruns the mounted slice.
    expect(mountedChatWindowCoversViewport(stationary, geometry, 14800, 600, 0)).toBe(true);
    expect(mountedChatWindowCoversViewport(stationary, geometry, 14800, 600, -500)).toBe(false);
    expect(mountedChatWindowCoversViewport(fastUp, geometry, 14500, 600, -500)).toBe(true);
    expect(mountedChatWindowCoversViewport(fastUp, geometry, 14000, 600, -500)).toBe(false);
    expect(mountedChatWindowCoversViewport(fastDown, geometry, 15500, 600, 500)).toBe(true);
    expect(mountedChatWindowCoversViewport(fastDown, geometry, 16000, 600, 500)).toBe(false);

    // Extreme velocity on dense rows stays strictly within CHAT_MOUNT_MAX and biases spare rows in the direction of motion.
    const dense = Array(300).fill(50);
    const extremeUp = mountedChatWindow({ heights: dense, scrollTop: 7500, viewportHeight: 600, velocity: -2000 });
    const extremeDown = mountedChatWindow({ heights: dense, scrollTop: 7500, viewportHeight: 600, velocity: 2000 });
    expect(extremeUp.end - extremeUp.start).toBeLessThanOrEqual(CHAT_MOUNT_MAX);
    expect(extremeDown.end - extremeDown.start).toBeLessThanOrEqual(CHAT_MOUNT_MAX);
    expect(150 - extremeUp.start).toBeGreaterThan(extremeUp.end - 162);
    expect(extremeDown.end - 162).toBeGreaterThan(150 - extremeDown.start);

    // Low speed (<= 25% viewport height) keeps symmetric overscan and CHAT_MOUNT_TARGET cap.
    const slow = mountedChatWindow({ heights: Array(220).fill(20), scrollTop: 800, viewportHeight: 500, velocity: -100 });
    expect(slow.end - slow.start).toBe(CHAT_MOUNT_TARGET);
  });
  it("tracks signed per-frame scroll velocity with instant attack, decay, reversal, and idle reset", () => {
    let state = nextChatScrollVelocity(null, 5000, 0);
    expect(state.velocity).toBe(0);

    // Instant attack on first fast upward flick frame after idle pause.
    state = nextChatScrollVelocity(state, 4500, 500);
    expect(state.velocity).toBe(-500);

    // Sustained flick keeps peak speed; deceleration decays smoothly (0.75x).
    state = nextChatScrollVelocity(state, 4000, 518);
    expect(state.velocity).toBe(-500);
    state = nextChatScrollVelocity(state, 3900, 536);
    expect(state.velocity).toBe(-375);

    // Direction reversal snaps immediately to new signed velocity.
    state = nextChatScrollVelocity(state, 4300, 554);
    expect(state.velocity).toBe(400);

    // Sub-frame micro-events accumulate within a frame without inflating across frames.
    let sub = nextChatScrollVelocity(null, 1000, 100);
    sub = nextChatScrollVelocity(sub, 1002, 101);
    sub = nextChatScrollVelocity(sub, 1005, 102);
    expect(sub.velocity).toBe(5);

    // Stationary frame resets velocity to zero.
    state = nextChatScrollVelocity(state, 4300, 572);
    expect(state.velocity).toBe(0);
  });
  it("distinguishes visible-viewport coverage (blank imminent) from proactive overscan coverage", () => {
    const geometry = new IndexedChatRowGeometry();
    for (let index = 0; index < 300; index++) geometry.append(100);
    // Mounted slice at scrollTop=15000, viewport=500 -> start=145 (14500), end=160 (16000).
    const window = mountedChatWindow({ heights: Array(300).fill(100), scrollTop: 15000, viewportHeight: 500 });
    // Moving to 14600 crosses the 50% proactive margin (14600 - 250 < 14500) so proactive coverage is false,
    // yet visible viewport [14600, 15100] is still fully covered by [14500, 16000] (no blank imminent).
    expect(mountedChatWindowCoversViewport(window, geometry, 14600, 500)).toBe(false);
    expect(mountedChatWindowCoversVisibleViewport(window, geometry, 14600, 500)).toBe(true);
    // Moving past the mounted edge leaves blank imminent at top or bottom.
    expect(mountedChatWindowCoversVisibleViewport(window, geometry, 14400, 500)).toBe(false);
    expect(mountedChatWindowCoversVisibleViewport(window, geometry, 15600, 500)).toBe(false);
    // Updating a mounted row height in geometry preserves accurate visible coverage without spurious misses.
    geometry.update(150, 60);
    expect(mountedChatWindowCoversVisibleViewport(window, geometry, 14600, 500)).toBe(true);
  });
});

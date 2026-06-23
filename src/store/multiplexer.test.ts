import { highestTitleLevel, muxStateEventAction } from "./multiplexer";
import { TitleLevel } from "../lib/types";

// ── highestTitleLevel ─────────────────────────────────────────────────

let passed = 0;
let failed = 0;

function assertTitle(label: string, actual: TitleLevel, expected: TitleLevel) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${expected}, got ${actual}`);
    failed++;
  }
}

function assertEqual<T>(label: string, actual: T, expected: T) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${expected}, got ${actual}`);
    failed++;
  }
}

console.log("highestTitleLevel:");

// Same level returns itself
assertTitle("Default == Default", highestTitleLevel(TitleLevel.Default, TitleLevel.Default), TitleLevel.Default);
assertTitle("User == User", highestTitleLevel(TitleLevel.User, TitleLevel.User), TitleLevel.User);
assertTitle("Agent == Agent", highestTitleLevel(TitleLevel.Agent, TitleLevel.Agent), TitleLevel.Agent);

// Higher wins over lower
assertTitle("User > Default", highestTitleLevel(TitleLevel.User, TitleLevel.Default), TitleLevel.User);
assertTitle("Default < User", highestTitleLevel(TitleLevel.Default, TitleLevel.User), TitleLevel.User);
assertTitle("User > Auto", highestTitleLevel(TitleLevel.User, TitleLevel.Auto), TitleLevel.User);
assertTitle("Auto < User", highestTitleLevel(TitleLevel.Auto, TitleLevel.User), TitleLevel.User);
assertTitle("Agent > Auto", highestTitleLevel(TitleLevel.Agent, TitleLevel.Auto), TitleLevel.Agent);
assertTitle("Auto < Agent", highestTitleLevel(TitleLevel.Auto, TitleLevel.Agent), TitleLevel.Agent);
assertTitle("Agent > Default", highestTitleLevel(TitleLevel.Agent, TitleLevel.Default), TitleLevel.Agent);
assertTitle("User > Agent", highestTitleLevel(TitleLevel.User, TitleLevel.Agent), TitleLevel.User);
assertTitle("Agent < User", highestTitleLevel(TitleLevel.Agent, TitleLevel.User), TitleLevel.User);

// Full chain: Default < Auto < Agent < User
assertTitle("Auto > Default", highestTitleLevel(TitleLevel.Auto, TitleLevel.Default), TitleLevel.Auto);
assertTitle("Default < Auto", highestTitleLevel(TitleLevel.Default, TitleLevel.Auto), TitleLevel.Auto);

console.log("\nmuxStateEventAction:");
assertEqual(
  "applies desktop mux snapshot",
  muxStateEventAction({ workspaces: [{ id: "w1" }] }),
  "apply-snapshot",
);
assertEqual(
  "applies empty desktop mux snapshot",
  muxStateEventAction({ workspaces: [] }),
  "apply-snapshot",
);
assertEqual(
  "refreshes on web typed SSE mux event",
  muxStateEventAction({ event: "mux_state_changed" }),
  "refresh",
);
assertEqual(
  "refreshes on web polling signal",
  muxStateEventAction(null),
  "refresh",
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

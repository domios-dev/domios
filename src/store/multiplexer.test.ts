import { highestTitleLevel } from "./multiplexer";
import { TitleLevel } from "../lib/types";

// ── highestTitleLevel ─────────────────────────────────────────────────

let passed = 0;
let failed = 0;

function assert(label: string, actual: TitleLevel, expected: TitleLevel) {
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
assert("Default == Default", highestTitleLevel(TitleLevel.Default, TitleLevel.Default), TitleLevel.Default);
assert("User == User", highestTitleLevel(TitleLevel.User, TitleLevel.User), TitleLevel.User);
assert("Agent == Agent", highestTitleLevel(TitleLevel.Agent, TitleLevel.Agent), TitleLevel.Agent);

// Higher wins over lower
assert("User > Default", highestTitleLevel(TitleLevel.User, TitleLevel.Default), TitleLevel.User);
assert("Default < User", highestTitleLevel(TitleLevel.Default, TitleLevel.User), TitleLevel.User);
assert("User > Auto", highestTitleLevel(TitleLevel.User, TitleLevel.Auto), TitleLevel.User);
assert("Auto < User", highestTitleLevel(TitleLevel.Auto, TitleLevel.User), TitleLevel.User);
assert("Agent > Auto", highestTitleLevel(TitleLevel.Agent, TitleLevel.Auto), TitleLevel.Agent);
assert("Auto < Agent", highestTitleLevel(TitleLevel.Auto, TitleLevel.Agent), TitleLevel.Agent);
assert("Agent > Default", highestTitleLevel(TitleLevel.Agent, TitleLevel.Default), TitleLevel.Agent);
assert("User > Agent", highestTitleLevel(TitleLevel.User, TitleLevel.Agent), TitleLevel.User);
assert("Agent < User", highestTitleLevel(TitleLevel.Agent, TitleLevel.User), TitleLevel.User);

// Full chain: Default < Auto < Agent < User
assert("Auto > Default", highestTitleLevel(TitleLevel.Auto, TitleLevel.Default), TitleLevel.Auto);
assert("Default < Auto", highestTitleLevel(TitleLevel.Default, TitleLevel.Auto), TitleLevel.Auto);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

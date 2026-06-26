// Run: npx tsx tests/playwright/helpers/canvas-figma-acceptance.test.ts

import assert from "node:assert/strict";
import {
  footprintEqual,
  footprintsStable,
  screenToWorld,
  screenTracksWorldScale,
  worldToScreen,
} from "./canvas-figma-acceptance";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn): void {
  tests.push([name, fn]);
}

test("screenToWorld divides by camera scale", () => {
  assert.deepEqual(screenToWorld({ w: 200, h: 100 }, 2), { w: 100, h: 50 });
});

test("worldToScreen multiplies by camera scale", () => {
  assert.deepEqual(worldToScreen({ w: 100, h: 50 }, 2), { w: 200, h: 100 });
});

test("footprintEqual tolerates 5% drift", () => {
  assert.equal(footprintEqual(200, 204), true);
  assert.equal(footprintEqual(200, 220), false);
});

test("footprintsStable requires matching world sizes across samples", () => {
  assert.equal(
    footprintsStable([
      { w: 180, h: 90 },
      { w: 182, h: 91 },
      { w: 179, h: 89 },
    ]),
    true,
  );
  assert.equal(
    footprintsStable([
      { w: 180, h: 90 },
      { w: 140, h: 90 },
    ]),
    false,
  );
});

test("screenTracksWorldScale links world and screen boxes", () => {
  const world = { w: 160, h: 80 };
  assert.equal(screenTracksWorldScale(world, { w: 320, h: 160 }, 2), true);
  assert.equal(screenTracksWorldScale(world, { w: 200, h: 160 }, 2), false);
});

async function main(): Promise<void> {
  let failures = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failures += 1;
      console.error(`  ✗ ${name}`);
      console.error(error);
    }
  }
  if (failures > 0) process.exit(1);
}

void main();

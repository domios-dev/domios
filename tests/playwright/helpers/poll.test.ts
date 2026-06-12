import assert from "node:assert/strict";
import { pollUntil } from "./poll";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn): void {
  tests.push([name, fn]);
}

test("pollUntil returns the first truthy value", async () => {
  let attempts = 0;
  const value = await pollUntil(
    () => {
      attempts += 1;
      return attempts === 3 ? "ready" : "";
    },
    500,
    5,
    "truthy value",
  );

  assert.equal(value, "ready");
  assert.equal(attempts, 3);
});

test("pollUntil reports timeout with the label", async () => {
  await assert.rejects(
    () => pollUntil(() => false, 20, 5, "never ready"),
    /pollUntil timed out after 20ms: never ready/,
  );
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

  if (failures > 0) {
    process.exit(1);
  }
}

void main();

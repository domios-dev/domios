// Focused coverage for the delayed-messages poll gating (perf2-composer-poll):
// an inactive composer must not run the periodic listDelayedMessages poll.
import { delayedPollIntervalMs } from "./ChatComposer";

type TestFn = () => void | Promise<void>;
const failures: string[] = [];
function test(name: string, fn: TestFn) {
  try {
    const r = fn();
    if (r instanceof Promise) throw new Error("sync tests only");
    console.log(`ok - ${name}`);
  } catch (e) {
    failures.push(name);
    console.error(`FAIL - ${name}:`, e);
  }
}

test("inactive composer schedules no delayed-messages poll", () => {
  if (delayedPollIntervalMs(false) !== null) {
    throw new Error("isActive=false must disable the periodic poll entirely");
  }
});

test("active composer polls at the 10s drift-insurance cadence", () => {
  if (delayedPollIntervalMs(true) !== 10_000) {
    throw new Error("isActive=true must poll at 10_000ms (events remain the primary path)");
  }
});

if (failures.length > 0) {
  process.exit(1);
}
console.log("ChatComposer poll-gating checks passed");

import { shouldUseTouchOptimizedUi } from "./useTouchOptimizedUi";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(!shouldUseTouchOptimizedUi(true, false, false), "a narrow desktop viewport must not enable touch controls");
assert(shouldUseTouchOptimizedUi(true, true, false), "a coarse pointer should enable touch controls");
assert(shouldUseTouchOptimizedUi(true, false, true), "a hoverless input should enable touch controls");
assert(!shouldUseTouchOptimizedUi(false, true, true), "native desktop should not enable the web touch key bar");

console.log("useTouchOptimizedUi tests: 4/4 passed");

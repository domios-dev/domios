import { defaultConfig } from "./useConfig";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const memory = defaultConfig().memory;
assert(memory?.extraction_model === "", "extraction model waits for the provider catalog");
assert(memory?.dreaming_model === "", "dreaming model waits for the provider catalog");
assert(memory?.worker_model === "", "worker model waits for the provider catalog");

console.log("✓ config has no pre-catalog model fallback");

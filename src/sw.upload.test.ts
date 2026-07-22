import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const source = readFileSync(fileURLToPath(new URL("./sw.ts", import.meta.url)), "utf8");

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(source.includes('/^\\/api\\//'), "API routes remain excluded from navigation caching");
assert(!source.includes('respondWith(fetch(event.request))'), "service worker never re-streams API request bodies");
assert(!source.includes('addEventListener("fetch"'), "Workbox navigation routing owns the only fetch interception");

console.log("service worker upload bypass test passed");

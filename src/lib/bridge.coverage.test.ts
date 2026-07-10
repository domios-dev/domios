/**
 * Bridge coverage guard — every literal invoke("<cmd>") in src/ must be
 * HTTP-mapped, canned, web-special, or declared native-only (WEB_UNSUPPORTED).
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { WEB_SPECIAL, WEB_UNSUPPORTED } from "./bridgeWebUnsupported";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const SRC = path.join(ROOT, "src");
const INVOKE_RE = /invoke(?:<[^>]*>)?\(\s*["']([a-z_][a-z0-9_]*)["']/g;

function walk(dir: string, out: string[]): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === "dist") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, out);
    } else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

function readBridgeSets(): { http: Set<string>; canned: Set<string> } {
  const bridge = fs.readFileSync(path.join(ROOT, "src/lib/bridge.ts"), "utf8");
  const httpBlock = bridge.match(/const HTTP_MAP[\s\S]*?^};/m)?.[0] ?? "";
  const cannedBlock = bridge.match(/const CANNED[\s\S]*?^};/m)?.[0] ?? "";
  const keyRe = /^\s+([a-z_][a-z0-9_]*):/gm;
  return {
    http: new Set([...httpBlock.matchAll(keyRe)].map((m) => m[1])),
    canned: new Set([...cannedBlock.matchAll(keyRe)].map((m) => m[1])),
  };
}

function scanInvokeLiterals(): Set<string> {
  const cmds = new Set<string>();
  for (const file of walk(SRC, [])) {
    const text = fs.readFileSync(file, "utf8");
    for (const m of text.matchAll(INVOKE_RE)) cmds.add(m[1]);
  }
  return cmds;
}

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("bridge coverage guard:");

const invoked = scanInvokeLiterals();
const { http, canned } = readBridgeSets();
const unsupported = new Set(Object.keys(WEB_UNSUPPORTED));
const special = new Set(Object.keys(WEB_SPECIAL));
const covered = new Set([...http, ...canned, ...unsupported, ...special]);

const unmapped = [...invoked].filter((c) => !covered.has(c)).sort();
assert(
  unmapped.length === 0
    ? "every literal invoke() command is covered"
    : `unmapped invoke command(s): ${unmapped.join(", ")}`,
  unmapped.length === 0,
);

for (const [cmd, reason] of Object.entries(WEB_UNSUPPORTED)) {
  assert(`WEB_UNSUPPORTED ${cmd} has justification`, reason.trim().length > 0);
}

const staleUnsupported = [...unsupported].filter((c) => !invoked.has(c)).sort();
assert(
  staleUnsupported.length === 0
    ? "no stale WEB_UNSUPPORTED entries"
    : `stale WEB_UNSUPPORTED (no longer invoked): ${staleUnsupported.join(", ")}`,
  staleUnsupported.length === 0,
);

const staleSpecial = [...special].filter((c) => !invoked.has(c)).sort();
assert(
  staleSpecial.length === 0
    ? "no stale WEB_SPECIAL entries"
    : `stale WEB_SPECIAL (no longer invoked): ${staleSpecial.join(", ")}`,
  staleSpecial.length === 0,
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);

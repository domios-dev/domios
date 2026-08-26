import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

import { appVersion, buildHash } from "./buildStamp";
import { __resetForTest, checkBuildMismatch } from "./buildMismatchGuard";

const STAMP_IDENTIFIERS = /(__BUILD_HASH__|__APP_VERSION__)/;
const STAMP_IDENTIFIERS_GLOBAL = /(^|[^.\w$])(__BUILD_HASH__|__APP_VERSION__)\b/g;
const STAMP_PROPERTY_READ = /\.\s*(__BUILD_HASH__|__APP_VERSION__)\b/;

const srcDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const stampSourcePath = path.join(srcDir, "lib", "buildStamp.ts");

let passed = 0;
let failed = 0;
function assert(label: string, cond: unknown) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

interface MutableGlobal {
  __BUILD_HASH__?: string;
  __APP_VERSION__?: string;
  fetch?: unknown;
}
const mutableGlobal = globalThis as unknown as MutableGlobal;

function setStamps(hash: string | undefined, version: string | undefined) {
  if (hash === undefined) delete mutableGlobal.__BUILD_HASH__;
  else mutableGlobal.__BUILD_HASH__ = hash;
  if (version === undefined) delete mutableGlobal.__APP_VERSION__;
  else mutableGlobal.__APP_VERSION__ = version;
}

function stubHealth(body: { build?: string; version?: string }) {
  mutableGlobal.fetch = async () => ({ ok: true, json: async () => body });
}

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "dist") sourceFiles(full, out);
      continue;
    }
    if (!entry.isFile() || !/\.tsx?$/.test(entry.name)) continue;
    if (/\.test\.tsx?$/.test(entry.name)) continue;
    out.push(full);
  }
  return out;
}

async function main() {
  console.log("buildStamp: accessor reads the substituted identifier:");
  setStamps("9f3ac21", "1.9.4");
  assert("buildHash() returns the supplied build stamp", buildHash() === "9f3ac21");
  assert("appVersion() returns the supplied version stamp", appVersion() === "1.9.4");

  console.log("\nbuildStamp: accessor falls back when the stamp was never injected:");
  setStamps(undefined, undefined);
  assert("buildHash() falls back to dev", buildHash() === "dev");
  assert("appVersion() falls back to dev", appVersion() === "dev");

  console.log("\nbuildStamp: the accessor source survives Vite `define` substitution:");
  const stampSource = readFileSync(stampSourcePath, "utf8");
  assert(
    "the accessor names the stamp identifiers at all",
    STAMP_IDENTIFIERS.test(stampSource),
  );
  const substituted = stampSource.replace(
    STAMP_IDENTIFIERS_GLOBAL,
    (_match, prefix: string) => `${prefix}"injected-by-define"`,
  );
  assert(
    "no stamp read survives define substitution (a property read would)",
    !STAMP_IDENTIFIERS.test(substituted),
  );

  console.log("\nbuildStamp: no consumer reads a stamp off an object:");
  const offenders = sourceFiles(srcDir).filter((file) =>
    STAMP_PROPERTY_READ.test(readFileSync(file, "utf8")),
  );
  assert(
    `every stamp read is a bare identifier (offenders: ${offenders.join(", ") || "none"})`,
    offenders.length === 0,
  );

  console.log("\nbuildMismatchGuard: consumes the accessor:");
  __resetForTest();
  setStamps(undefined, undefined);
  stubHealth({ build: "0000000", version: "1.9.4" });
  assert(
    "an uninjected frontend stays silent instead of warning against every daemon",
    (await checkBuildMismatch("http://fixture")) === null,
  );

  __resetForTest();
  setStamps("9f3ac21", undefined);
  stubHealth({ build: "0000000" });
  const mismatch = await checkBuildMismatch("http://fixture");
  assert("a disagreeing daemon raises the banner", mismatch !== null);
  assert("the banner carries the injected stamp", mismatch?.frontendHash === "9f3ac21");
  assert("the banner attributes the mismatch to the build", mismatch?.reason === "build");

  __resetForTest();
  setStamps("9f3ac21", undefined);
  stubHealth({ build: "9f3ac21" });
  assert(
    "an agreeing daemon raises nothing",
    (await checkBuildMismatch("http://fixture")) === null,
  );

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();

import { appVersion, buildHash } from "./buildStamp";
import { __resetForTest, checkBuildMismatch } from "./buildMismatchGuard";
import { frontendApiContract } from "./frontendApiContract";

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

function stubHealth(body: { build?: string; version?: string; apiVersion?: number }) {
  mutableGlobal.fetch = async () => ({ ok: true, json: async () => body });
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

  console.log("\nbuildMismatchGuard: consumes the accessor:");
  __resetForTest();
  setStamps(undefined, undefined);
  stubHealth({ build: "0000000", version: "1.9.4", apiVersion: frontendApiContract.minApiVersion });
  assert(
    "a compatible daemon accepts an uninjected frontend identity",
    (await checkBuildMismatch("http://fixture")) === null,
  );

  __resetForTest();
  setStamps("9f3ac21", undefined);
  stubHealth({ build: "0000000", apiVersion: 1 });
  const mismatch = await checkBuildMismatch("http://fixture");
  assert("an incompatible API raises the banner", mismatch !== null);
  assert("the banner carries the injected stamp", mismatch?.frontendHash === "9f3ac21");
  assert("the banner attributes the mismatch to the API", mismatch?.reason === "api");

  __resetForTest();
  setStamps("9f3ac21", undefined);
  stubHealth({ build: "independent-daemon", apiVersion: frontendApiContract.minApiVersion });
  assert(
    "an independent compatible daemon raises nothing",
    (await checkBuildMismatch("http://fixture")) === null,
  );

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();

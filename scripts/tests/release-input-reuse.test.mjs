import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("../../build.sh", import.meta.url), "utf8");
const preparation = source.match(/^prepare_release_bundle_inputs\(\) \{\n[\s\S]*?^\}/m)?.[0];

function commands(env = {}, captureEnvironment = false) {
  assert.ok(preparation);
  return execFileSync("bash", ["-c", `
set -e
pnpm() { printf 'pnpm'; printf ' <%s>' "$@"; printf '\\n'; }
node() { if [ \"$1\" = -p ]; then command node \"$@\"; return; fi; printf 'node'; printf ' <%s>' "$@"; printf '\\n'; }
cargo() { ${captureEnvironment ? `bash -c 'printf "cargo-deployment-target=<%s>\\n" "$MACOSX_DEPLOYMENT_TARGET"';` : ''} printf 'cargo'; printf ' <%s>' "$@"; printf '\\n'; }
${preparation}
prepare_release_bundle_inputs
${captureEnvironment ? `bash -c 'printf "deployment-target=<%s>\\n" "$MACOSX_DEPLOYMENT_TARGET"'` : ''}
`], { encoding: "utf8", env: { ...process.env, CARGO_BUILD_TARGET: "", CODETERM_RELEASE_FEATURES: "", ...env } })
    .trim().split("\n");
}

test("release preparation builds the packaging feature graph before staging the real embed", () => {
  const calls = commands();
  assert.deepEqual(calls, [
    "pnpm <build>",
    "cargo <build> <--release> <--timings> <--manifest-path> <cli/Cargo.toml>",
    "node <scripts/bundle-cli.js>",
    "node <scripts/prepare-embed-stub.js>",
    "cargo <build> <--release> <--timings> <--manifest-path> <src-tauri/Cargo.toml> <--features> <embeddings,tauri/custom-protocol> <--bin> <codeterm> <--bin> <codeterm-embed>",
    "node <scripts/bundle-embed.js>",
  ]);
});

test("cross compilation and explicit release features reach both preparation builds", () => {
  const calls = commands({ CARGO_BUILD_TARGET: "x86_64-unknown-linux-gnu", CODETERM_RELEASE_FEATURES: "feature-fixture" });
  const builds = calls.filter(call => call.startsWith("cargo"));
  assert.equal(builds.length, 2);
  for (const call of builds) assert.ok(call.includes("<--target> <x86_64-unknown-linux-gnu>"));
  assert.ok(builds[1].includes("<--features> <feature-fixture,tauri/custom-protocol>"));
});

test("CLI build entrypoints leave source timestamps under Cargo ownership", () => {
  assert.doesNotMatch(source, /touch cli\/src\/main\.rs|touch-cli-main/);
  const prep = readFileSync(new URL("../dev-prep.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(prep, /touch\("cli\/src\/main\.rs"\)|utimesSync/);
});

test("release preparation exports the packaging deployment target to child processes", () => {
  const config = JSON.parse(readFileSync(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const calls = commands({ MACOSX_DEPLOYMENT_TARGET: "fixture-inherited-target" }, true);
  const cargoTargets = calls.filter(call => call.startsWith("cargo-deployment-target="));
  assert.deepEqual(cargoTargets, Array(2).fill(`cargo-deployment-target=<${config.bundle.macOS.minimumSystemVersion}>`));
  assert.equal(calls.at(-1), `deployment-target=<${config.bundle.macOS.minimumSystemVersion}>`);
});

test('release entrypoint configures the wrapper before spawning compiler children', () => {
  const entrypoint = source.match(/^  release-inputs\)\n([\s\S]*?)^    ;;/m)?.[1];
  assert.ok(entrypoint);
  const result = execFileSync('bash', ['-c', `
set -e
ct_configure_sccache() { export RUSTC_WRAPPER=fixture-wrapper; }
ensure_deps() { :; }
prepare_release_bundle_inputs() { bash -c 'printf "%s" "$RUSTC_WRAPPER"'; }
${entrypoint}
`], { encoding: 'utf8', env: { ...process.env, RUSTC_WRAPPER: '' } });
  assert.equal(result, 'fixture-wrapper');
});

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";

const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
const source = readFileSync(new URL("../../build.sh", import.meta.url), "utf8").replaceAll("\r\n", "\n");
const preparation = source.match(/^prepare_release_bundle_inputs\(\) \{\n[\s\S]*?^\}/m)?.[0];

function commands(env = {}, captureEnvironment = false, mode = "") {
  assert.ok(preparation);
  return execFileSync(bash, ["-c", `
set -e
pnpm() { printf 'pnpm'; printf ' <%s>' "$@"; printf '\\n'; }
node() { if [ \"$1\" = -p ]; then command node \"$@\"; return; fi; printf 'node'; printf ' <%s>' "$@"; printf '\\n'; }
cargo() { ${captureEnvironment ? `bash -c 'printf "cargo-deployment-target=<%s>\\n" "$MACOSX_DEPLOYMENT_TARGET"';` : ''} printf 'cargo'; printf ' <%s>' "$@"; printf '\\n'; }
${preparation}
prepare_release_bundle_inputs ${mode}
${captureEnvironment ? `bash -c 'printf "deployment-target=<%s>\\n" "$MACOSX_DEPLOYMENT_TARGET"'` : ''}
`], { encoding: "utf8", env: { ...process.env, CARGO_BUILD_TARGET: "", CODETERM_RELEASE_FEATURES: "", ...env } })
    .trim().split("\n");
}

test("release preparation builds the packaging feature graph without an embedding sidecar", () => {
  const calls = commands();
  assert.deepEqual(calls, [
    "pnpm <build>",
    "cargo <build> <--release> <--timings> <--manifest-path> <cli/Cargo.toml>",
    "node <scripts/bundle-cli.js>",
    "cargo <build> <--release> <--timings> <--manifest-path> <src-tauri/Cargo.toml> <--features> <embeddings,tauri/custom-protocol> <--bin> <codeterm>",
  ]);
});

test("packaging prepares checked frontend and CLI inputs without compiling the app twice", () => {
  const calls = commands({}, false, "bundle");
  assert.deepEqual(calls, [
    "pnpm <build>",
    "cargo <build> <--release> <--timings> <--manifest-path> <cli/Cargo.toml>",
    "node <scripts/bundle-cli.js>",
    "node <scripts/prepare-bundle-resource-stubs.mjs> <assert-package-ready>",
  ]);
});

test("each packaging entrypoint leaves native compilation to Tauri's final config", () => {
  for (const verb of ["release-bundle", "build", "install-local"]) {
    const entrypoint = source.match(new RegExp(`^  ${verb}\\)\\n([\\s\\S]*?)^    ;;`, "m"))?.[1];
    assert.ok(entrypoint, verb);
    assert.match(entrypoint, /^    prepare_release_bundle_inputs bundle$/m, verb);
    assert.match(entrypoint, /pnpm tauri build/, verb);
  }
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

test('release bundle forwards local packaging flags without splitting config paths', () => {
  const entrypoint = source.match(/^  release-bundle\)\n([\s\S]*?)^    ;;/m)?.[1];
  assert.ok(entrypoint);
  const calls = execFileSync(bash, ['-c', `
set -e
ensure_deps() { :; }
prepare_release_bundle_inputs() { :; }
pnpm() { printf 'pnpm'; printf ' <%s>' "$@"; }
set -- release-bundle --bundles deb --config '/tmp/local bundle.json' --ci
${entrypoint}
`], { encoding: 'utf8', env: { ...process.env, CARGO_BUILD_TARGET: '', CODETERM_RELEASE_FEATURES: '' } });
  assert.equal(calls, 'pnpm <tauri> <build> <--features> <embeddings> <--bundles> <deb> <--config> </tmp/local bundle.json> <--ci>');
});

test('fresh packaging checks source and selects the existing release target before preparing inputs', () => {
  const entrypoint = source.match(/^  release-bundle\)\n([\s\S]*?)^    ;;/m)?.[1];
  const calls = execFileSync(bash, ['-c', `
set -e
require_release_source() { printf 'source-check\\n'; }
prepare_release_target() { export CARGO_TARGET_DIR=fixture-fresh-target; printf 'fresh-target\\n'; }
ensure_deps() { :; }
prepare_release_bundle_inputs() { printf 'prepare <%s> <%s>\\n' "$1" "$CARGO_TARGET_DIR"; }
pnpm() { printf 'pnpm'; printf ' <%s>' "$@"; }
set -- release-bundle --fresh-target --bundles deb --config '/tmp/local bundle.json' --ci
${entrypoint}
`], { encoding: 'utf8', env: { ...process.env, CARGO_BUILD_TARGET: '', CODETERM_RELEASE_FEATURES: '' } });
  assert.equal(calls, 'source-check\nfresh-target\nprepare <bundle> <fixture-fresh-target>\npnpm <tauri> <build> <--features> <embeddings> <--bundles> <deb> <--config> </tmp/local bundle.json> <--ci>');
});

test('fresh packaging refuses dirty source before allocating a target or compiling', () => {
  const entrypoint = source.match(/^  release-bundle\)\n([\s\S]*?)^    ;;/m)?.[1];
  assert.throws(() => execFileSync(bash, ['-c', `
set -e
require_release_source() { return 23; }
prepare_release_target() { printf 'unexpected target'; }
ensure_deps() { printf 'unexpected install'; }
prepare_release_bundle_inputs() { printf 'unexpected compile'; }
pnpm() { printf 'unexpected package'; }
set -- release-bundle --fresh-target
${entrypoint}
`], { encoding: 'utf8' }), error => error.status === 23 && error.stdout === '');
});

test("release preparation exports the packaging deployment target to child processes", () => {
  const config = JSON.parse(readFileSync(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const calls = commands({ MACOSX_DEPLOYMENT_TARGET: "fixture-inherited-target" }, true);
  const cargoTargets = calls.filter(call => call.startsWith("cargo-deployment-target="));
  assert.deepEqual(cargoTargets, Array(2).fill(`cargo-deployment-target=<${config.bundle.macOS.minimumSystemVersion}>`));
  assert.equal(calls.at(-1), `deployment-target=<${config.bundle.macOS.minimumSystemVersion}>`);
});

test('release entrypoints configure the wrapper before spawning compiler children', () => {
  const gateway = source.match(/^enter_cargo_gateway\(\) \{\n[\s\S]*?^\}/m)?.[0];
  const gatedVerbs = source.match(/^case "\$CMD" in\n  ([^\n]*)\)\n    enter_cargo_gateway /m)?.[1].split('|');
  assert.ok(gateway);
  for (const verb of ['release-inputs', 'release-bundle']) {
    assert.ok(gatedVerbs?.includes(verb), verb);
    const entrypoint = source.match(new RegExp(`^  ${verb}\\)\\n([\\s\\S]*?)^    ;;`, 'm'))?.[1];
    assert.ok(entrypoint, verb);
    const result = execFileSync(bash, ['-c', `
set -e
enter_build_gateway() { :; }
ct_configure_sccache() { export RUSTC_WRAPPER=fixture-wrapper; }
ensure_deps() { :; }
prepare_release_bundle_inputs() { bash -c 'printf "%s" "$RUSTC_WRAPPER"'; }
pnpm() { :; }
${gateway}
enter_cargo_gateway ${verb}
${entrypoint}
`], { encoding: 'utf8', env: { ...process.env, RUSTC_WRAPPER: '' } });
    assert.equal(result, 'fixture-wrapper', verb);
  }
});

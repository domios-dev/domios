import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";

const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
const source = readFileSync(new URL("../../build.sh", import.meta.url), "utf8").replaceAll("\r\n", "\n");
const preparation = source.match(/^prepare_release_bundle_inputs\(\) \{\n[\s\S]*?^\}/m)?.[0];

test('hosted native Windows arm64 preserves the initialized Visual Studio environment', () => {
  const native = source.match(/^ct_msvc_native_arm64_env\(\) \{[\s\S]*?^\}/m)?.[0];
  const guard = source.match(/^ct_assert_msvc_lib_arch\(\) \{[\s\S]*?^\}/m)?.[0];
  assert.ok(native && guard);
  const calls = execFileSync(bash, ['-s'], { encoding: 'utf8', input: `
set -e
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin/Hostarm64/arm64"
touch "$work/bin/Hostarm64/arm64/cl.exe" "$work/bin/Hostarm64/arm64/lib.exe" "$work/bin/Hostarm64/arm64/link.exe"
cygpath() { printf '%s' "$2"; }
${guard}
${native}
export VCToolsInstallDir="$work/" VSCMD_ARG_HOST_ARCH=arm64 VSCMD_ARG_TGT_ARCH=arm64 LIB='C:/vc/lib/arm64;C:/sdk/um/arm64'
ct_msvc_native_arm64_env
test "$CC" = "$work/bin/Hostarm64/arm64/cl.exe"
test "$AR_aarch64_pc_windows_msvc" = "$work/bin/Hostarm64/arm64/lib.exe"
test "\${PATH%%:*}" = "$work/bin/Hostarm64/arm64"
if VSCMD_ARG_HOST_ARCH=x64 ct_msvc_native_arm64_env 2>/dev/null; then exit 91; fi
if LIB='C:/vc/lib/x64' ct_msvc_native_arm64_env 2>/dev/null; then exit 92; fi
printf 'native tools and architecture refusals verified'
` });
  assert.equal(calls, 'native tools and architecture refusals verified');
  const setup = source.match(/^setup_env\(\) \{[\s\S]*?^\}/m)?.[0];
  assert.match(setup, /release-arm64-compile[\s\S]*?ct_msvc_native_arm64_env[\s\S]*?return/);
});

test('hosted arm64 freezes the handoff in its build session after unsigned bundle-only packaging', () => {
  const entrypoint = source.match(/^  release-arm64-compile\)\n([\s\S]*?)^    ;;/m)?.[1];
  assert.ok(entrypoint);
  const calls = execFileSync(bash, ['-c', `
set -e
ensure_deps() { :; }
prepare_release_bundle_inputs() { printf 'prepare <%s> <%s>\\n' "$1" "$CODETERM_RELEASE_COMMON_STAGE"; }
bundle_common_stage() { printf 'bundle unsigned <%s>\\n' "$CODETERM_RELEASE_UNSIGNED"; }
node() { printf 'node'; printf ' <%s>' "$@"; }
set -- release-arm64-compile v1.2.3 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa '/tmp/archive with spaces.tar'
${entrypoint}
`], { encoding: 'utf8', env: { ...process.env, CARGO_BUILD_TARGET: 'aarch64-pc-windows-msvc', TAURI_SIGNING_PRIVATE_KEY: '', TAURI_SIGNING_PRIVATE_KEY_PATH: '', AWS_SECRET_ACCESS_KEY: '' } });
  assert.equal(calls, 'prepare <bundle> <1>\nbundle unsigned <1>\nnode <scripts/release-arm64-handoff.mjs> <pack> <aarch64-pc-windows-msvc> <v1.2.3> <aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa> </tmp/archive with spaces.tar>');
  const shared = source.match(/^bundle_common_stage\(\) \{[\s\S]*?^\}/m)?.[0];
  assert.match(shared, /unsigned_config=\(--config '\{"bundle":\{"createUpdaterArtifacts":false\}\}'\)/);
  assert.match(shared, /tauri build --no-bundle[^\n]*"\$\{unsigned_config\[@\]\}"/);
  for (const bundles of ['nsis', 'deb,appimage']) assert.match(shared, new RegExp(`--bundles ${bundles} "\\$\\{unsigned_config\\[@\\]\\}"`));
});


test('macOS native compilation cannot overwrite the signed server with the sidecar stub', () => {
  for (const file of ['../../.agents/skills/github-release/scripts/build-mac.sh', '../release-macos-x86_64.sh']) {
    const text = readFileSync(new URL(file, import.meta.url), 'utf8');
    const config = text.match(/pnpm tauri build --no-bundle[\s\S]*?--config '(\{[^\n]+\})'/)?.[1];
    assert.ok(config, file);
    assert.deepEqual(JSON.parse(config).bundle.externalBin, [], file);
    assert.match(text, /node scripts\/release-macos-bundle\.mjs/);
  }
  const assembly = readFileSync(new URL('../release-macos-bundle.mjs', import.meta.url), 'utf8');
  assert.match(assembly, /'--config', '\{"bundle":\{"createUpdaterArtifacts":false\}\}'/);
  assert.doesNotMatch(assembly, /externalBin/);
});



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
    "cargo <build> <--release> <--timings> <--manifest-path> <src-tauri/Cargo.toml> <--features> <tauri/custom-protocol> <--bin> <codeterm>",
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

test('shared-stage release places the built frontend before native compilation', () => {
  const frontend = preparation.indexOf('  pnpm build');
  const stage = preparation.indexOf('stage-build-resources');
  const cli = preparation.indexOf('cargo build --release --timings --manifest-path cli/Cargo.toml');
  const server = preparation.indexOf('cargo build --release --manifest-path src-tauri/Cargo.toml -p domios-server');
  assert.ok(frontend >= 0 && frontend < stage && stage < cli && cli < server);
  assert.equal(preparation.match(/\bpnpm build\b/g)?.length, 1);
});

test("each packaging entrypoint leaves native compilation to Tauri's final config", () => {
  for (const verb of ["release-bundle", "build", "install-local"]) {
    const entrypoint = source.match(new RegExp(`^  ${verb}\\)\\n([\\s\\S]*?)^    ;;`, "m"))?.[1];
    assert.ok(entrypoint, verb);
    assert.match(entrypoint, /^    prepare_release_bundle_inputs bundle$/m, verb);
    assert.match(entrypoint, /pnpm tauri build|bundle_common_stage/, verb);
  }
  const commonStage = source.match(/^bundle_common_stage\(\) \{[\s\S]*?^\}/m)?.[0];
  assert.match(commonStage, /pnpm tauri build --no-bundle/);
  assert.match(commonStage, /release-stage\.mjs stage-full/);
  assert.match(commonStage, /pnpm tauri bundle --no-binary-patching/);
  assert.match(commonStage, /release-macos-bundle\.mjs/);
});

test("cross compilation and explicit release features reach both preparation builds", () => {
  const calls = commands({ CARGO_BUILD_TARGET: "x86_64-unknown-linux-gnu", CODETERM_RELEASE_FEATURES: "feature-fixture" });
  const builds = calls.filter(call => call.startsWith("cargo"));
  assert.equal(builds.length, 2);
  for (const call of builds) assert.ok(call.includes("<--target> <x86_64-unknown-linux-gnu>"));
  assert.ok(builds[1].includes("<--features> <tauri/custom-protocol,feature-fixture>"));
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
  assert.equal(calls, 'pnpm <tauri> <build> <--bundles> <deb> <--config> </tmp/local bundle.json> <--ci>');
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
  assert.equal(calls, 'source-check\nfresh-target\nprepare <bundle> <fixture-fresh-target>\npnpm <tauri> <build> <--bundles> <deb> <--config> </tmp/local bundle.json> <--ci>');
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
    const failure = execFileSync(bash, ['-c', `
enter_build_gateway() { :; }
ct_configure_sccache() { return 42; }
${gateway}
if enter_cargo_gateway ${verb}; then exit 99; else printf '%s' "$?"; fi
`], { encoding: 'utf8' });
    assert.equal(failure, '42', verb);
  }
});

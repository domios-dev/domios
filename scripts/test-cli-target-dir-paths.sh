#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

assert_eq() {
  local expected="$1"
  local actual="$2"
  local label="$3"
  if [ "$actual" != "$expected" ]; then
    echo "FAIL: $label" >&2
    echo "expected: $expected" >&2
    echo "actual:   $actual" >&2
    exit 1
  fi
}

build_release_dir() {
  (
    cd "$repo_root"
    env -u CODETERM_API_PORT "$@" CODETERM_TEST_PRINT_CLI_RELEASE_DIR=1 ./build.sh cli
  )
}

assert_eq "cli/target/release" \
  "$(build_release_dir -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET)" \
  "build.sh default target dir"

assert_eq "cli/target/aarch64-apple-darwin/release" \
  "$(build_release_dir -u CARGO_TARGET_DIR CARGO_BUILD_TARGET=aarch64-apple-darwin)" \
  "build.sh default target dir with CARGO_BUILD_TARGET"

assert_eq "/tmp/codeterm-cli-target/release" \
  "$(build_release_dir -u CARGO_BUILD_TARGET CARGO_TARGET_DIR=/tmp/codeterm-cli-target)" \
  "build.sh CARGO_TARGET_DIR"

assert_eq "/tmp/codeterm-cli-target/x86_64-pc-windows-msvc/release" \
  "$(build_release_dir CARGO_TARGET_DIR=/tmp/codeterm-cli-target CARGO_BUILD_TARGET=x86_64-pc-windows-msvc)" \
  "build.sh CARGO_TARGET_DIR with CARGO_BUILD_TARGET"

cd "$repo_root"
node <<'NODE'
const assert = require("assert");
const path = require("path");
const { resolveCliSource } = require("./scripts/bundle-cli.js");

const root = path.resolve("/repo");
const target = path.resolve("/tmp/codeterm-cli-target");

assert.strictEqual(
  resolveCliSource({}, root, "codeterm"),
  path.join(root, "cli", "target", "release", "codeterm")
);
assert.strictEqual(
  resolveCliSource({ CARGO_BUILD_TARGET: "aarch64-apple-darwin" }, root, "codeterm"),
  path.join(root, "cli", "target", "aarch64-apple-darwin", "release", "codeterm")
);
assert.strictEqual(
  resolveCliSource({ CARGO_TARGET_DIR: target }, root, "codeterm"),
  path.join(target, "release", "codeterm")
);
assert.strictEqual(
  resolveCliSource(
    { CARGO_TARGET_DIR: target, CARGO_BUILD_TARGET: "x86_64-pc-windows-msvc" },
    root,
    "codeterm.exe"
  ),
  path.join(target, "x86_64-pc-windows-msvc", "release", "codeterm.exe")
);
NODE

echo "CLI target-dir path tests passed"

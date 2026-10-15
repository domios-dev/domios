import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { findViolations } from '../check-archive-portable-test-paths.mjs';

const MANIFEST = 'env!("CARGO_MANIFEST_DIR")';
const BIN = 'env!("CARGO_BIN_EXE_codeterm")';

function fixture(files) {
  const root = mkdtempSync(join(tmpdir(), 'domios-archive-paths-'));
  for (const [path, body] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), body);
  }
  return root;
}

function violations(files) {
  const root = fixture(files);
  try { return findViolations(root); } finally { rmSync(root, { recursive: true, force: true }); }
}

test('integration test binary path is rejected', () => {
  assert.deepEqual(violations({ 'cli/tests/run.rs': `fn bin() { Command::new(${BIN}); }\n` }), [
    `cli/tests/run.rs:1: ${BIN}`,
  ]);
});

test('runtime manifest path in an inline cfg(test) module is rejected, production is not', () => {
  const lib = `pub fn root() -> PathBuf { PathBuf::from(${MANIFEST}) }\n#[cfg(test)]\nmod tests {\n    fn root() -> PathBuf {\n        PathBuf::from(${MANIFEST})\n    }\n}\n`;
  assert.deepEqual(violations({ 'src-tauri/crates/a/src/lib.rs': lib }), [
    `src-tauri/crates/a/src/lib.rs:5: ${MANIFEST}`,
  ]);
});

test('a cfg(test) statement is rejected while its cfg(not(test)) twin is allowed', () => {
  const body = `fn root() -> PathBuf {\n    #[cfg(test)]\n    let root = PathBuf::from(${MANIFEST});\n    #[cfg(not(test))]\n    let root = PathBuf::from(${MANIFEST});\n    root\n}\n`;
  assert.deepEqual(violations({ 'src-tauri/crates/a/src/view.rs': body }), [
    `src-tauri/crates/a/src/view.rs:3: ${MANIFEST}`,
  ]);
});

test('a file declared as a cfg(test) module and its descendants are test code', () => {
  const found = violations({
    'src-tauri/crates/a/src/lib.rs': '#[cfg(test)]\nmod parity;\n',
    'src-tauri/crates/a/src/parity.rs': `fn a() { let _ = ${MANIFEST}; }\n`,
    'src-tauri/crates/a/src/parity/deep.rs': `fn b() { let _ = ${MANIFEST}; }\n`,
    'src-tauri/crates/a/src/other.rs': `fn c() { let _ = ${MANIFEST}; }\n`,
  });
  assert.deepEqual(found, [
    `src-tauri/crates/a/src/parity.rs:1: ${MANIFEST}`,
    `src-tauri/crates/a/src/parity/deep.rs:1: ${MANIFEST}`,
  ]);
});

test('compile-time embedding, helpers, build scripts and comments are allowed', () => {
  const found = violations({
    'src-tauri/crates/a/src/tests.rs': `const S: &str = include_str!(concat!(\n    ${MANIFEST},\n    "/fixtures/a.txt"\n));\n// ${MANIFEST}\nconst T: &str = "${MANIFEST.replace(/"/g, '\\"')}";\n`,
    'src-tauri/crates/a/src/test_support.rs': `pub fn dir() -> PathBuf { PathBuf::from(${MANIFEST}) }\n`,
    'cli/tests/support/codeterm_bin.rs': `pub fn bin() -> PathBuf { PathBuf::from(${BIN}) }\n`,
    'src-tauri/crates/a/build.rs': `fn main() { let _ = ${MANIFEST}; }\n`,
  });
  assert.deepEqual(found, []);
});

test('CLI exits non-zero and names the offending line', () => {
  const root = fixture({ 'src-tauri/crates/a/src/x_tests.rs': `fn a() { let _ = ${MANIFEST}; }\n` });
  try {
    const script = fileURLToPath(new URL('../check-archive-portable-test-paths.mjs', import.meta.url));
    const run = spawnSync(process.execPath, [script, '--root', root], { encoding: 'utf8' });
    assert.equal(run.status, 1);
    assert.match(run.stderr, /src-tauri\/crates\/a\/src\/x_tests\.rs:1/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

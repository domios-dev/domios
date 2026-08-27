/**
 * WHAT: every provider the stub can wear owns its own surface and its own cast.
 * WHY: a stub that boots wearing another provider's banner makes detection
 *      fingerprint the wrong provider, and the test still reports green.
 * ORACLE: banner bytes and the cast map, plus the replayer's real stdout.
 */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { DEFAULT_CASTS, castOutput, defaultCastFor } from "./replay-cast.mjs";
import { KNOWN_STUB_PROVIDERS, STUB_MARKER, styleFor } from "./replay-style.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(HERE, "..", "..", "..", "..");

/** Structural boot fragments each provider's detector keys off. */
const PROVIDER_FINGERPRINTS = {
  claude: "Claude Code",
  codex: "OpenAI Codex",
  opencode: "opencode",
  cursor: "Cursor Agent",
};

test("every styled provider has a recorded cast and vice versa", () => {
  assert.deepEqual(
    [...KNOWN_STUB_PROVIDERS].sort(),
    Object.keys(DEFAULT_CASTS).sort(),
  );
});

test("cursor is one of them", () => {
  assert.ok(KNOWN_STUB_PROVIDERS.includes("cursor"));
  assert.ok(defaultCastFor("cursor"));
});

test("each default cast file exists and is a non-trivial recording", () => {
  for (const [provider, castPath] of Object.entries(DEFAULT_CASTS)) {
    const resolved = path.join(REPO, castPath);
    assert.ok(existsSync(resolved), `${provider} cast missing: ${castPath}`);
    const bytes = castOutput(readFileSync(resolved, "utf8"), 8_000_000);
    assert.ok(bytes.length > 1_000, `${provider} cast is trivially short`);
  }
});

test("each style carries only its own provider fingerprint", () => {
  for (const provider of KNOWN_STUB_PROVIDERS) {
    const banner = styleFor(provider, "sonnet").banner;
    assert.ok(
      banner.includes(PROVIDER_FINGERPRINTS[provider]),
      `${provider} banner must carry its own fingerprint`,
    );
    for (const [other, fingerprint] of Object.entries(PROVIDER_FINGERPRINTS)) {
      if (other === provider) continue;
      assert.ok(
        !banner.includes(fingerprint),
        `${provider} banner must not carry ${other}'s fingerprint`,
      );
    }
  }
});

test("the model the caller supplies round-trips into the banner", () => {
  for (const provider of KNOWN_STUB_PROVIDERS) {
    assert.match(styleFor(provider, "pw-model-token").banner, /pw-model-token/);
  }
});

test("every style stays probe-visible to the stub gate", () => {
  for (const provider of [...KNOWN_STUB_PROVIDERS, "aider"]) {
    assert.ok(styleFor(provider, null).banner.includes(STUB_MARKER));
  }
});

// Negative control for the defect: before cursor had a style, styleFor fell
// through to Claude and the stub booted wearing a Claude banner.
test("an unstyled provider names itself and borrows nobody's fingerprint", () => {
  const banner = styleFor("aider", "x").banner;
  assert.ok(banner.includes("aider"), "banner must name the provider it stands in for");
  for (const fingerprint of Object.values(PROVIDER_FINGERPRINTS)) {
    assert.ok(!banner.includes(fingerprint), `unstyled banner leaked ${fingerprint}`);
  }
  assert.equal(defaultCastFor("aider"), null);
});

function runReplay(provider, extra = {}) {
  // An ambient CODETERM_STUB_CAST would silently replace the map under test.
  const env = { ...process.env, ...extra };
  delete env.CODETERM_STUB_CAST;
  return execFileSync(
    process.execPath,
    [path.join(HERE, "replay.mjs"), "--stub-provider", provider, "--", provider, "--model", "x"],
    { input: "", encoding: "utf8", maxBuffer: 64 * 1024 * 1024, env },
  );
}

test("a cursor stub boots as cursor, not as Claude", () => {
  const stdout = runReplay("cursor");
  assert.ok(stdout.includes("Cursor Agent"), "cursor boot must announce Cursor Agent");
  assert.ok(!stdout.includes("Claude Code"), "cursor boot must not wear the Claude banner");
});

test("a cursor stub replays its recorded cursor cast", () => {
  const stdout = runReplay("cursor", { CODETERM_STUB_REPLAY_BYTE_CAP: "8000000" });
  const recorded = castOutput(
    readFileSync(path.join(REPO, DEFAULT_CASTS.cursor), "utf8"),
    8_000_000,
  );
  assert.ok(recorded.length > 1_000);
  assert.ok(stdout.includes(recorded), "cursor boot must embed the recorded cursor transcript");
});

// Negative control: a provider with no cast must say so, not boot silently.
test("a cast-less provider says it has no recording", () => {
  const stdout = runReplay("aider");
  assert.match(stdout, /no recorded cast for provider aider/);
});

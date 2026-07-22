// Parity gate: the bytes the stub emits for its boot transcript are exactly
// the recorded real-session bytes, so detection/parsing downstream of the PTY
// pipeline see byte-identical input. Run: node --test replay.parity.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(HERE, "..", "..", "..", "..");

const CASTS = {
  claude:
    "testbench/replays/claude/geometry_smoke_v1-20260621-geometry-smoke-v2-claude-geometry_smoke_v1-100x30/raw.cast",
  codex:
    "testbench/replays/codex/codex_prompt_idle_regression_v1-20260621-codex-prompt-idle-v1/raw.cast",
  opencode:
    "testbench/replays/opencode/geometry_smoke_v1-20260621-geometry-smoke-v2-opencode-geometry_smoke_v1-100x30/raw.cast",
};

function castBytes(castPath) {
  const lines = readFileSync(path.join(REPO, castPath), "utf8").split("\n").slice(1);
  let out = "";
  for (const line of lines) {
    if (!line.trim()) continue;
    const [, kind, data] = JSON.parse(line);
    if (kind === "o") out += data;
  }
  return out;
}

function encodeClaudeCwd(cwd) {
  return Array.from(String(cwd || ""))
    .map((c) => (/[\p{L}\p{N}]/u.test(c) ? c : "-"))
    .join("");
}

for (const [provider, castPath] of Object.entries(CASTS)) {
  test(`${provider}: stub boot replays the recorded real transcript byte-for-byte`, () => {
    const stdout = execFileSync(
      process.execPath,
      [path.join(HERE, "replay.mjs"), "--stub-provider", provider, "--", provider, "--model", "x"],
      { input: "", encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
    );
    const recorded = castBytes(castPath);
    assert.ok(recorded.length > 1000, "recorded cast is non-trivial");
    assert.ok(
      stdout.includes(recorded),
      `${provider} replay output must embed the recorded transcript verbatim`,
    );
  });
}

test("claude: scripted turns persist under the native provider project dir", () => {
  const cwd = mkdtempSync(path.join(os.tmpdir(), "pw_stub_agent_under_score_"));
  const nonce = `PARITY-${Date.now()}`;
  try {
    execFileSync(
      process.execPath,
      [
        path.join(HERE, "replay.mjs"),
        "--stub-provider",
        "claude",
        "--",
        "claude",
        "--model",
        "x",
        "--",
        `STUB_ECHO ${nonce}`,
      ],
      { cwd, input: "", encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
    );
    const encoded = encodeClaudeCwd(realpathSync(cwd));
    const dir = path.join(os.homedir(), ".claude", "projects", encoded);
    assert.ok(existsSync(dir), `expected native provider dir ${dir}`);
    const sessionFiles = new Set(
      existsSync(dir) ? readdirSync(dir).filter((name) => name.endsWith(".jsonl")) : [],
    );
    assert.ok(sessionFiles.size > 0, `expected a session jsonl in ${dir}`);
    const hit = Array.from(sessionFiles).some((file) =>
      readFileSync(path.join(dir, file), "utf8").includes(`ack ${nonce}`),
    );
    assert.ok(hit, "scripted assistant reply must be readable from the native provider path");
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});

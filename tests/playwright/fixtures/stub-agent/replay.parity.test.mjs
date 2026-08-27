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
import { DEFAULT_CASTS, castOutput } from "./replay-cast.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(HERE, "..", "..", "..", "..");

// The replayer truncates at CODETERM_STUB_REPLAY_BYTE_CAP; lift it past the
// largest default cast so the gate compares whole transcripts, not prefixes.
const FULL_TRANSCRIPT_CAP = 8_000_000;

function castBytes(castPath) {
  return castOutput(readFileSync(path.join(REPO, castPath), "utf8"), FULL_TRANSCRIPT_CAP);
}

// An ambient CODETERM_STUB_CAST would silently replace the map under test.
function replayEnv(extra = {}) {
  const env = { ...process.env, ...extra };
  delete env.CODETERM_STUB_CAST;
  return env;
}

function encodeClaudeCwd(cwd) {
  return Array.from(String(cwd || ""))
    .map((c) => (/[\p{L}\p{N}]/u.test(c) ? c : "-"))
    .join("");
}

for (const [provider, castPath] of Object.entries(DEFAULT_CASTS)) {
  test(`${provider}: stub boot replays the recorded real transcript byte-for-byte`, () => {
    const stdout = execFileSync(
      process.execPath,
      [path.join(HERE, "replay.mjs"), "--stub-provider", provider, "--", provider, "--model", "x"],
      {
        input: "",
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
        env: replayEnv({ CODETERM_STUB_REPLAY_BYTE_CAP: String(FULL_TRANSCRIPT_CAP) }),
      },
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

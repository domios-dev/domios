import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import {
  CURSOR_AGENT_BIN_ENV,
  cursorAgentDirs,
  discoverCursorAgent,
  discoverExecutable,
} from "./agent-binary";

function fakeFs(...present: string[]): (candidate: string) => boolean {
  const set = new Set(present);
  return (candidate) => set.has(candidate);
}

test("finds a POSIX binary on PATH", () => {
  const hit = path.join("/opt/tools", "cursor-agent");
  assert.equal(
    discoverExecutable("cursor-agent", {
      platform: "linux",
      env: { PATH: `/nope:/opt/tools` },
      exists: fakeFs(hit),
    }),
    hit,
  );
});

test("finds a Windows binary through PATHEXT", () => {
  const hit = path.join("C:\\tools", "cursor-agent.CMD");
  assert.equal(
    discoverExecutable("cursor-agent", {
      platform: "win32",
      env: { PATH: "C:\\other;C:\\tools", PATHEXT: ".COM;.EXE;.CMD" },
      exists: fakeFs(hit),
    }),
    hit,
  );
});

// Case-insensitive NTFS hides this; a case-sensitive directory (1803+), WSL or a
// network mount does not, and the fallout is a silent skip.
test("an uppercase PATHEXT still finds the lowercase name installers write", () => {
  const hit = path.join("C:\\tools", "cursor-agent.exe");
  assert.equal(
    discoverExecutable("cursor-agent", {
      platform: "win32",
      env: { PATH: "C:\\tools", PATHEXT: ".EXE;.CMD" },
      exists: fakeFs(hit),
    }),
    hit,
  );
});

test("a lowercase PATHEXT still finds an uppercase name", () => {
  const hit = path.join("C:\\tools", "cursor-agent.EXE");
  assert.equal(
    discoverExecutable("cursor-agent", {
      platform: "win32",
      env: { PATH: "C:\\tools", PATHEXT: ".exe;.cmd" },
      exists: fakeFs(hit),
    }),
    hit,
  );
});

test("falls back to the extra per-user install dirs", () => {
  const hit = path.join("/home/pw/.local/bin", "cursor-agent");
  assert.equal(
    discoverExecutable("cursor-agent", {
      platform: "linux",
      env: { PATH: "/usr/bin" },
      extraDirs: ["/home/pw/.local/bin"],
      exists: fakeFs(hit),
    }),
    hit,
  );
});

test("an explicit env override wins and round-trips the supplied path", () => {
  const supplied = "/custom/place/cursor-agent";
  assert.equal(
    discoverExecutable("cursor-agent", {
      envVar: CURSOR_AGENT_BIN_ENV,
      platform: "linux",
      env: { [CURSOR_AGENT_BIN_ENV]: supplied, PATH: "/usr/bin" },
      exists: fakeFs(supplied, "/usr/bin/cursor-agent"),
    }),
    supplied,
  );
});

// ── negative controls ──

test("a set-but-missing override raises instead of silently skipping", () => {
  assert.throws(
    () =>
      discoverExecutable("cursor-agent", {
        envVar: CURSOR_AGENT_BIN_ENV,
        platform: "linux",
        env: { [CURSOR_AGENT_BIN_ENV]: "/gone/cursor-agent" },
        exists: fakeFs(),
      }),
    /does not exist/,
  );
});

test("an absent binary resolves to null, never to a guessed path", () => {
  assert.equal(
    discoverExecutable("cursor-agent", {
      platform: "linux",
      env: { PATH: "/usr/bin:/usr/local/bin" },
      exists: fakeFs(),
    }),
    null,
  );
});

// The defect: the spec hardcoded /Users/kandrey/.local/bin/cursor-agent, so a
// Windows machine with cursor-agent installed still skipped, forever.
test("a Windows install is discovered where the hardcoded macOS path could not be", () => {
  const macOnlyPath = "/Users/kandrey/.local/bin/cursor-agent";
  const installed = path.join(cursorAgentDirs("C:\\Users\\pw")[0], "cursor-agent.exe");
  const exists = fakeFs(installed);
  assert.equal(exists(macOnlyPath), false);
  assert.equal(
    discoverCursorAgent({
      platform: "win32",
      env: { PATH: "C:\\Windows", PATHEXT: ".EXE;.CMD", USERPROFILE: "C:\\Users\\pw" },
      exists,
    }),
    installed,
  );
});

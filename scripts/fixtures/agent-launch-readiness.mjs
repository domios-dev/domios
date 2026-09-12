#!/usr/bin/env node
/**
 * Launch-readiness oracle fixture — stands in for a provider CLI so a live dev
 * daemon can be driven through both launch-readiness outcomes without a real
 * provider account and without answering any trust or permission gate.
 *
 * Invocation is built by src-tauri/src/agent/stub_launch.rs:
 *   node scripts/fixtures/agent-launch-readiness.mjs --stub-provider <id> -- <provider argv...>
 *
 * Mode selection is a file, read once at start, so the daemon environment is
 * set before the daemon starts and the mode is chosen per spawn:
 *   CODETERM_STUB_AGENT_CMD          = node scripts/fixtures/agent-launch-readiness.mjs
 *   CODETERM_READINESS_FIXTURE_CONTROL = <control directory>
 *   <control>/mode    = "exit-before-readiness" | "slow-ready"   (required, read at start)
 *   <control>/started = written by this fixture once it is running (observe this, not the terminal)
 *   <control>/release = create it to release "slow-ready" into readiness
 *
 * Windows PowerShell:
 *   $env:CODETERM_STUB_AGENT_CMD = 'node scripts/fixtures/agent-launch-readiness.mjs'
 *   $env:CODETERM_READINESS_FIXTURE_CONTROL = "$env:TEMP\codeterm-launch-readiness-fixture"
 *   New-Item -ItemType Directory -Force $env:CODETERM_READINESS_FIXTURE_CONTROL
 *   Set-Content -NoNewline "$env:CODETERM_READINESS_FIXTURE_CONTROL\mode" 'exit-before-readiness'
 *   codeterm agent spawn claude --title launch-readiness-fixture
 *
 * bash / zsh (macOS, Linux):
 *   export CODETERM_STUB_AGENT_CMD='node scripts/fixtures/agent-launch-readiness.mjs'
 *   export CODETERM_READINESS_FIXTURE_CONTROL="${TMPDIR:-/tmp}/codeterm-launch-readiness-fixture"
 *   mkdir -p "$CODETERM_READINESS_FIXTURE_CONTROL"
 *   printf 'slow-ready' > "$CODETERM_READINESS_FIXTURE_CONTROL/mode"
 *   codeterm agent spawn claude --title launch-readiness-fixture
 *   # then, only after <control>/started appears:
 *   : > "$CODETERM_READINESS_FIXTURE_CONTROL/release"
 *
 * exit-before-readiness: emit the started marker, then exit nonzero before any
 * readiness or session-binding evidence. The daemon must learn this as one
 * bounded structured launch failure naming the tab and its launch attempt.
 *
 * slow-ready: emit the started marker, wait for the release file as a
 * filesystem event (never a timer, never a poll), then report the
 * provider-neutral session binding the shared observer treats as readiness and
 * stay alive. Readiness inside the bounded deadline must not be a failure.
 */
import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, watch, writeFileSync } from "node:fs";
import path from "node:path";

const MODES = ["exit-before-readiness", "slow-ready"];
const USAGE = `agent-launch-readiness fixture: set CODETERM_READINESS_FIXTURE_CONTROL to a directory whose "mode" file contains one of ${MODES.join(" | ")}`;

function parseArgv(argv) {
  const sep = argv.indexOf("--");
  const own = sep === -1 ? argv : argv.slice(0, sep);
  const marker = own.indexOf("--stub-provider");
  return { provider: (marker !== -1 && own[marker + 1]) || "claude" };
}

function out(line) {
  process.stdout.write(`${line}\r\n`);
}

const { provider } = parseArgv(process.argv.slice(2));
const control = process.env.CODETERM_READINESS_FIXTURE_CONTROL || "";
const tabId = process.env.CODETERM_TAB_ID || "";
const attemptNonce = process.env.CODETERM_SESSION_BINDING_NONCE || "";
const apiUrl = (process.env.CODETERM_API_URL || "").replace(/\/$/, "");
const apiToken = process.env.CODETERM_API_TOKEN || "";

if (!control) {
  out(USAGE);
  process.exit(64);
}

const modeFile = path.join(control, "mode");
const startedFile = path.join(control, "started");
const releaseFile = path.join(control, "release");

const mode = (() => {
  try {
    return readFileSync(modeFile, "utf8").trim();
  } catch {
    return "";
  }
})();

if (!MODES.includes(mode)) {
  out(`${USAGE} (read ${JSON.stringify(mode)} from ${modeFile})`);
  process.exit(64);
}

// The started marker is a file as well as a line: the oracle observes the file
// so it never has to scrape terminal text to know the fixture is running.
function announceStarted() {
  out(`launch-readiness-fixture: started mode=${mode} provider=${provider} tab=${tabId} attempt_nonce=${attemptNonce ? "present" : "absent"}`);
  try {
    mkdirSync(control, { recursive: true });
    writeFileSync(startedFile, `${mode}\n${tabId}\n`);
  } catch {
    /* the stdout marker still stands */
  }
}

async function reportSessionBinding() {
  if (!apiUrl || !apiToken || !tabId) {
    out("launch-readiness-fixture: no daemon callback in the environment; readiness cannot be reported");
    return false;
  }
  try {
    const response = await fetch(`${apiUrl}/api/tabs/${tabId}/hook/session_start`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${apiToken}`,
      },
      body: JSON.stringify({
        session_id: randomUUID(),
        source: "startup",
        provider,
        binding_nonce: attemptNonce || undefined,
      }),
      signal: AbortSignal.timeout(10000),
    });
    out(`launch-readiness-fixture: session binding reported status=${response.status}`);
    return response.ok;
  } catch (error) {
    out(`launch-readiness-fixture: session binding failed (${String(error).slice(0, 120)})`);
    return false;
  }
}

// Resolves on the release file's arrival. `watch` is the event source; the one
// existence check covers a release created before the watcher was armed.
function awaitRelease() {
  return new Promise((resolve) => {
    let settled = false;
    const done = (watcher) => {
      if (settled) return;
      settled = true;
      watcher?.close();
      resolve();
    };
    let watcher;
    try {
      watcher = watch(control, (_event, name) => {
        if (!name || path.basename(String(name)) === "release") {
          if (existsSync(releaseFile)) done(watcher);
        }
      });
      watcher.on("error", () => done(watcher));
    } catch {
      watcher = undefined;
    }
    if (existsSync(releaseFile)) done(watcher);
  });
}

function stayAlive() {
  process.stdin.resume();
  process.stdin.on("end", () => process.exit(0));
  process.on("SIGINT", () => process.exit(0));
  process.on("SIGTERM", () => process.exit(0));
}

announceStarted();

if (mode === "exit-before-readiness") {
  out("launch-readiness-fixture: exiting before any readiness or session-binding evidence");
  process.exit(3);
}

await awaitRelease();
out("launch-readiness-fixture: released");
await reportSessionBinding();
out("launch-readiness-fixture: ready");
stayAlive();

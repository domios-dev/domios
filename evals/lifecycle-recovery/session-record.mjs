#!/usr/bin/env node

// A stub stands in for a provider, and a provider proves session ownership by its own per-session record.
import { existsSync, mkdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const RECORD_ROOTS = {
  claude: () => join(homedir(), ".claude", "projects"),
};

function canonicalCwd(cwd) {
  try {
    return realpathSync.native(cwd);
  } catch {
    return cwd.replace(/[\\/]+$/, "");
  }
}

export function projectDir(provider, cwd) {
  const root = RECORD_ROOTS[provider];
  if (!root) throw new Error(`no session record layout for stub provider ${provider}`);
  const encoded = [...canonicalCwd(cwd)].map((char) => (/[\p{L}\p{N}]/u.test(char) ? char : "-")).join("");
  return join(root(), encoded);
}

export function recordPath(provider, cwd, sessionId) {
  return join(projectDir(provider, cwd), `${sessionId}.jsonl`);
}

export function writeSessionRecord(provider, cwd, sessionId) {
  mkdirSync(projectDir(provider, cwd), { recursive: true });
  writeFileSync(recordPath(provider, cwd, sessionId), `${JSON.stringify({ type: "user", sessionId, cwd })}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, provider, cwd, sessionId] = process.argv.slice(2);
  if (command === "exists") {
    process.exitCode = existsSync(recordPath(provider, cwd, sessionId)) ? 0 : 1;
  } else if (command === "remove") {
    rmSync(projectDir(provider, cwd), { recursive: true, force: true });
  } else {
    process.stderr.write(`unknown session-record command: ${command}\n`);
    process.exitCode = 2;
  }
}

import { type HostIoSteps } from "@codeterm/plugin-sdk";
import type { LaunchSessionDetection, LaunchSessionEvidence } from "@codeterm/plugin-sdk";
import { effectiveReadPath } from "../../provider-shared/readHome";

export const PROCESS_REGISTRY_DIR_ENV = "CODETERM_CLAUDE_PROCESS_REGISTRY_DIR";

function registryDir(): string | null {
  const override = host.envGet(PROCESS_REGISTRY_DIR_ENV);
  if (override) return override;
  const sep = host.platform() === "windows" ? "\\" : "/";
  const home = host.homeDir();
  return effectiveReadPath(host.envGet("CLAUDE_CONFIG_DIR"), home ? home + sep + ".claude" + sep + "sessions" : null, "sessions", sep);
}

function registryStartRefusal(recorded: string, token: string, modifiedMs?: number): string | null {
  if (!token) return "missing_live_start";
  if (recorded === token) return null;
  if (!/^\d+$/.test(token)) return "invalid_live_start";
  const micros = Number(token);
  if (!Number.isSafeInteger(micros)) return "invalid_live_start";
  const start = new Date(Math.floor(micros / 1_000_000) * 1_000);
  const weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const time = [start.getUTCHours(), start.getUTCMinutes(), start.getUTCSeconds()]
    .map(value => String(value).padStart(2, "0")).join(":");
  // Claude inherits C/UTC from the launch so ps lstart cannot repeat a DST fold.
  const expected = `${weekdays[start.getUTCDay()]} ${months[start.getUTCMonth()]} ${String(start.getUTCDate()).padStart(2, " ")} ${time} ${start.getUTCFullYear()}`;
  if (recorded !== expected) return "start_mismatch";
  if (modifiedMs === undefined) return "missing_record_mtime";
  return modifiedMs <= Math.floor(micros / 1_000) ? "stale_record" : null;
}

export function registrySessionId(raw: string, pid: number, startToken: string, modifiedMs?: number): string | null {
  const result = validateRegistrySession(raw, pid, startToken, modifiedMs);
  return "sessionId" in result ? result.sessionId : null;
}

function validateRegistrySession(raw: string, pid: number, startToken: string, modifiedMs?: number):
  { sessionId: string } | { refusal: string } {
  let record: Record<string, unknown>;
  try {
    record = JSON.parse(raw);
  } catch (_) {
    return { refusal: "invalid_json" };
  }
  if (!record || typeof record !== "object" || record.pid !== pid) return { refusal: "pid_mismatch" };
  // A child Claude run by the agent itself is not this tab's interactive session.
  if (typeof record.kind === "string" && record.kind !== "interactive") return { refusal: "non_interactive" };
  if (typeof record.procStart !== "string") return { refusal: "missing_recorded_start" };
  const refusal = registryStartRefusal(record.procStart.trim(), startToken, modifiedMs);
  if (refusal) return { refusal };
  const sessionId = typeof record.sessionId === "string" ? record.sessionId.trim() : "";
  return sessionId ? { sessionId } : { refusal: "missing_session_id" };
}

export function* detectClaudeLaunchSession(evidence: LaunchSessionEvidence): HostIoSteps<LaunchSessionDetection | null> {
  const dir = registryDir();
  if (!dir) {
    host.log("debug", "[claude-pid-registry] refused: registry_directory_unavailable");
    return null;
  }
  if (!evidence?.processes?.length) {
    host.log("debug", "[claude-pid-registry] refused: process_tree_empty");
    return null;
  }
  const platform = host.platform();
  const sep = platform === "windows" ? "\\" : "/";
  const processes = evidence.processes.slice().sort((a, b) => (a.depth - b.depth) || (a.pid - b.pid));
  for (let i = 0; i < processes.length; i++) {
    const proc = processes[i];
    const name = proc.pid + ".json";
    const path = dir + sep + name;
    const raw = yield* host.io.fs.readFile(path);
    if (typeof raw !== "string") {
      logRefusal(proc.pid, "record_unavailable");
      continue;
    }
    const token = String(proc.startToken || "");
    let result = validateRegistrySession(raw, proc.pid, token);
    if ("refusal" in result && result.refusal === "missing_record_mtime" && platform === "macos") {
      try {
        const entries = yield* host.io.fs.readDir(dir);
        const modifiedMs = entries.find(entry => entry.name === name && entry.isFile)?.modifiedMs;
        result = validateRegistrySession(raw, proc.pid, token, modifiedMs ?? undefined);
        // A replacement between the content read and the directory read must not lend its time.
        if ("sessionId" in result && raw !== (yield* host.io.fs.readFile(path))) {
          result = { refusal: "record_changed" };
        }
      } catch (error) {
        if (!(error instanceof RangeError)) throw error;
        result = { refusal: "registry_listing_limit" };
      }
    }
    if ("sessionId" in result) return { sessionId: result.sessionId, source: "pid_registry" };
    logRefusal(proc.pid, result.refusal);
  }
  return null;
}

function logRefusal(pid: number, reason: string) {
  host.log("debug", `[claude-pid-registry] pid=${pid} refused: ${reason}`);
}

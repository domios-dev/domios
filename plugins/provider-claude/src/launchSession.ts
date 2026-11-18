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

function registryStartMatches(recorded: string, token: string, modifiedMs?: number): boolean {
  if (!token) return false;
  if (recorded === token) return true;
  if (modifiedMs === undefined || !/^\d+$/.test(token)) return false;
  const micros = Number(token);
  if (!Number.isSafeInteger(micros) || modifiedMs <= Math.floor(micros / 1_000)) return false;
  const start = new Date(Math.floor(micros / 1_000_000) * 1_000);
  const weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const time = [start.getUTCHours(), start.getUTCMinutes(), start.getUTCSeconds()]
    .map(value => String(value).padStart(2, "0")).join(":");
  // Claude inherits C/UTC from the launch so ps lstart cannot repeat a DST fold.
  return recorded === `${weekdays[start.getUTCDay()]} ${months[start.getUTCMonth()]} ${String(start.getUTCDate()).padStart(2, " ")} ${time} ${start.getUTCFullYear()}`;
}

export function registrySessionId(raw: string, pid: number, startToken: string, modifiedMs?: number): string | null {
  let record: Record<string, unknown>;
  try {
    record = JSON.parse(raw);
  } catch (_) {
    return null;
  }
  if (!record || typeof record !== "object" || record.pid !== pid) return null;
  // A child Claude run by the agent itself is not this tab's interactive session.
  if (typeof record.kind === "string" && record.kind !== "interactive") return null;
  if (typeof record.procStart !== "string") return null;
  if (!registryStartMatches(record.procStart.trim(), startToken, modifiedMs)) return null;
  const sessionId = typeof record.sessionId === "string" ? record.sessionId.trim() : "";
  return sessionId || null;
}

export function* detectClaudeLaunchSession(evidence: LaunchSessionEvidence): HostIoSteps<LaunchSessionDetection | null> {
  const dir = registryDir();
  if (!dir || !evidence || !Array.isArray(evidence.processes)) return null;
  const platform = host.platform();
  const sep = platform === "windows" ? "\\" : "/";
  const processes = evidence.processes.slice().sort((a, b) => (a.depth - b.depth) || (a.pid - b.pid));
  for (let i = 0; i < processes.length; i++) {
    const proc = processes[i];
    const name = proc.pid + ".json";
    const path = dir + sep + name;
    const raw = yield* host.io.fs.readFile(path);
    if (typeof raw !== "string") continue;
    const token = String(proc.startToken || "");
    const sessionId = registrySessionId(raw, proc.pid, token);
    if (sessionId) return { sessionId, source: "pid_registry" };
    if (platform !== "macos") continue;
    try {
      const entries = yield* host.io.fs.readDir(dir);
      const modifiedMs = entries.find(entry => entry.name === name && entry.isFile)?.modifiedMs;
      const freshSession = registrySessionId(raw, proc.pid, token, modifiedMs ?? undefined);
      // A replacement between the content read and the directory read must not lend its time.
      if (freshSession && raw === (yield* host.io.fs.readFile(path))) {
        return { sessionId: freshSession, source: "pid_registry" };
      }
    } catch (error) {
      if (!(error instanceof RangeError)) throw error;
    }
  }
  return null;
}

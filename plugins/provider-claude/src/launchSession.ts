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

function registryStartMatches(recorded: string, token: string, macos: boolean): boolean {
  if (!token) return false;
  if (recorded === token) return true;
  if (!macos || !/^\d+$/.test(token)) return false;
  const micros = Number(token);
  if (!Number.isSafeInteger(micros)) return false;
  const start = new Date(Math.floor(micros / 1_000_000) * 1_000);
  const weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const time = [start.getUTCHours(), start.getUTCMinutes(), start.getUTCSeconds()]
    .map(value => String(value).padStart(2, "0")).join(":");
  // Claude inherits C/UTC from the launch so ps lstart cannot repeat a DST fold.
  return recorded === `${weekdays[start.getUTCDay()]} ${months[start.getUTCMonth()]} ${String(start.getUTCDate()).padStart(2, " ")} ${time} ${start.getUTCFullYear()}`;
}

export function registrySessionId(raw: string, pid: number, startToken: string, macos = false): string | null {
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
  if (!registryStartMatches(record.procStart.trim(), startToken, macos)) return null;
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
    const raw = (yield* host.io.fs.readFile(dir + sep + proc.pid + ".json"));
    if (typeof raw !== "string") continue;
    const sessionId = registrySessionId(raw, proc.pid, String(proc.startToken || ""), platform === "macos");
    if (sessionId) return { sessionId, source: "pid_registry" };
  }
  return null;
}

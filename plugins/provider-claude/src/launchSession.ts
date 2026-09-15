// A registry record binds only when its pid is in this tab's process tree and its
// recorded start equals the live process start (mirrors agent_claude launch_session.rs).
import type { LaunchSessionDetection, LaunchSessionEvidence } from "@codeterm/plugin-sdk";

export const PROCESS_REGISTRY_DIR_ENV = "CODETERM_CLAUDE_PROCESS_REGISTRY_DIR";

function registryDir(): string | null {
  const override = host.envGet(PROCESS_REGISTRY_DIR_ENV);
  if (override) return override;
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".claude" + sep + "sessions";
}

export function registrySessionId(raw: string, pid: number, startToken: string): string | null {
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
  if (!startToken || record.procStart.trim() !== startToken) return null;
  const sessionId = typeof record.sessionId === "string" ? record.sessionId.trim() : "";
  return sessionId || null;
}

export function detectClaudeLaunchSession(evidence: LaunchSessionEvidence): LaunchSessionDetection | null {
  const dir = registryDir();
  if (!dir || !evidence || !Array.isArray(evidence.processes)) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  const processes = evidence.processes.slice().sort((a, b) => (a.depth - b.depth) || (a.pid - b.pid));
  for (let i = 0; i < processes.length; i++) {
    const proc = processes[i];
    const raw = host.fs.readFile(dir + sep + proc.pid + ".json");
    if (typeof raw !== "string") continue;
    const sessionId = registrySessionId(raw, proc.pid, String(proc.startToken || ""));
    if (sessionId) return { sessionId, source: "pid_registry" };
  }
  return null;
}

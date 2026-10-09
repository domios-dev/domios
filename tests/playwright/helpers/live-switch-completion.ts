import { watch } from "node:fs";
import path from "node:path";
import { records, turnEvidence, type Evidence } from "./live-model-switch";

export function completedTurn(provider: string, evidence: Evidence, marker: string) {
  const turn = turnEvidence(provider, evidence, marker);
  if (provider === "claude" && !turn.raw.some(row => row.type === "assistant" && row.message?.stop_reason === "end_turn" &&
    JSON.stringify(row.message.content).includes(marker))) throw new Error(`Claude long turn has no completed assistant ${marker}`);
  if (provider === "codex" && !turn.raw.some(row => row.type === "event_msg" && row.payload?.type === "task_complete" &&
    row.payload.last_agent_message?.includes(marker))) throw new Error(`Codex long turn has no exact task_complete ${marker}`);
  return turn;
}

export function waitForCompletedTurn(provider: string, initial: Evidence, marker: string, timeoutMs: number,
  onSnapshot?: (evidence: Evidence) => Promise<void>): Promise<Evidence> {
  return new Promise((resolve, reject) => {
    const signal = AbortSignal.timeout(timeoutMs);
    let loading = false;
    let dirty = false;
    let finished = false;
    let lastError: unknown;
    const finish = (error?: Error, evidence?: Evidence) => {
      if (finished) return;
      finished = true;
      watcher.close();
      signal.removeEventListener("abort", expired);
      if (error) reject(error);
      else resolve(evidence!);
    };
    const expired = () => finish(new Error(`Exact completed own assistant ${marker} absent within ${timeoutMs}ms: ${String(lastError)}`));
    const inspect = async () => {
      if (finished) return;
      dirty = true;
      if (loading) return;
      loading = true;
      try {
        while (dirty && !finished) {
          dirty = false;
          try {
            const evidence = await records(provider, initial.session, initial.path);
            await onSnapshot?.(evidence);
            completedTurn(provider, evidence, marker);
            finish(undefined, evidence);
          } catch (error) { lastError = error; }
        }
      } finally { loading = false; }
    };
    const basename = path.basename(initial.path);
    const watcher = watch(path.dirname(initial.path), (_event, filename) => {
      if (!filename || [basename, `${basename}-wal`, `${basename}-shm`].includes(filename.toString())) void inspect();
    });
    watcher.on("error", error => finish(error));
    signal.addEventListener("abort", expired, { once: true });
    void inspect();
  });
}

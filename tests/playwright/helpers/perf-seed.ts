import type { ApiClient } from "./api-client";
import { injectCast, CAST } from "./inject-cast";
import { pollUntil } from "./poll";
import { stubPaneOutput } from "./stub-gate";

const STUB_PROVIDER = "claude";
// 30s (old proven stub-gate value) covers cold first-spawn PTY warmup; charged to
// the worker-fixture timeout, never the per-test --timeout=30000 measurement window.
const READY_CAP_MS = 30_000;

type SpawnedPane = { pane_id: string };

export interface PerfSeed {
  active: boolean;
  api: ApiClient;
  write: { front: string };
  history: { front: string; history: string; tail: string };
  cold: { front: string };
  churn: { front: string; churn: string };
}

// Active intent comes from the runner env, NOT a memoized probe that swallows a
// transient false; present CODETERM_STUB_AGENT_CMD means seed, never skip.
export function stubEnvActive(env: NodeJS.ProcessEnv = process.env): boolean {
  return (env.CODETERM_STUB_AGENT_CMD ?? "").trim().length > 0;
}

export const STUB_ENV_ABSENT_SKIP = "requires CODETERM_STUB_AGENT_CMD; real providers are forbidden";

// Cache TRUE only: a proven-active run may memoize, but false/errors never stick.
let cachedActiveTrue = false;

// Throws on banner timeout; a stuck/absent replayer must FAIL seeding, never skip.
export async function awaitStubBanner(api: ApiClient, paneId: string, label: string, capMs = READY_CAP_MS): Promise<void> {
  await pollUntil(async () => (await stubPaneOutput(api, paneId)).includes("stub replay"), capMs, 250, `${label} stub banner`);
}

async function spawnStub(api: ApiClient, title: string): Promise<string> {
  const spawned = await api.post<SpawnedPane>("/agents/spawn", { agent: STUB_PROVIDER, title });
  await awaitStubBanner(api, spawned.pane_id, title);
  return spawned.pane_id;
}

async function awaitPtyReady(api: ApiClient, paneId: string): Promise<void> {
  await pollUntil(
    async () => {
      try {
        await api.get(`/panes/${paneId}/output`);
        return true;
      } catch {
        return false;
      }
    },
    READY_CAP_MS,
    250,
    `pane ${paneId} PTY ready`,
  );
}

// Front-loads every expensive spawn/inject/readiness for all four hard metrics.
export async function preparePerfSeed(api: ApiClient): Promise<PerfSeed> {
  const active = cachedActiveTrue || stubEnvActive();
  if (!active) {
    return {
      active,
      api,
      write: { front: "" },
      history: { front: "", history: "", tail: "" },
      cold: { front: "" },
      churn: { front: "", churn: "" },
    };
  }

  // Env present: seed directly; any spawn/banner/readiness failure throws (never skips).
  const writeFront = await spawnStub(api, "pw-perf-write-render");

  const anchorFront = await spawnStub(api, "pw-perf-anchor-front");
  const anchorHistory = await spawnStub(api, "pw-perf-anchor-history");
  await awaitPtyReady(api, anchorHistory);
  await injectCast(api, { paneId: anchorHistory, castPath: CAST.claudeSystemsDesignLongChatIdle, instant: true });
  const tail = `PWANCHOR${Date.now()}`;
  await api.post(`/panes/${anchorHistory}/send`, { text: `STUB_ECHO ${tail}`, enter: true });
  await pollUntil(
    async () => (await stubPaneOutput(api, anchorHistory)).includes(`ack ${tail}`),
    READY_CAP_MS,
    250,
    "long history tail enters the PTY buffer",
  );

  const coldFront = await spawnStub(api, "pw-perf-cold-interactive");

  const ipcFront = await spawnStub(api, "pw-perf-ipc-front");
  const ipcChurn = await spawnStub(api, "pw-perf-ipc-churn");
  await awaitPtyReady(api, ipcChurn);
  await injectCast(api, { paneId: ipcChurn, castPath: CAST.claudeSystemsDesignLongChatIdle, speed: 8, classify: true });

  cachedActiveTrue = true;
  return {
    active,
    api,
    write: { front: writeFront },
    history: { front: anchorFront, history: anchorHistory, tail },
    cold: { front: coldFront },
    churn: { front: ipcFront, churn: ipcChurn },
  };
}

import type { ApiClient } from "./api-client";
import { injectCast, CAST } from "./inject-cast";
import { pollUntil } from "./poll";
import { isStubTransportActive, stubPaneOutput } from "./stub-gate";

const STUB_PROVIDER = "claude";
const READY_CAP_MS = 8_000;

type SpawnedPane = { pane_id: string };

export interface PerfSeed {
  active: boolean;
  api: ApiClient;
  write: { front: string };
  history: { front: string; history: string; tail: string };
  cold: { front: string };
  churn: { front: string; churn: string };
}

async function spawnStub(api: ApiClient, title: string): Promise<string> {
  const spawned = await api.post<SpawnedPane>("/agents/spawn", { agent: STUB_PROVIDER, title });
  await pollUntil(
    async () => (await stubPaneOutput(api, spawned.pane_id)).includes("stub replay"),
    READY_CAP_MS,
    250,
    `${title} stub banner`,
  );
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
  const active = await isStubTransportActive(api);
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

  return {
    active,
    api,
    write: { front: writeFront },
    history: { front: anchorFront, history: anchorHistory, tail },
    cold: { front: coldFront },
    churn: { front: ipcFront, churn: ipcChurn },
  };
}

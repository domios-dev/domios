/**
 * WHAT: stub agent replayer drives the REAL spawn/detection/parse pipeline.
 * WHY: golden agent-flow tests convert to this transport; it must behave like
 *      a live provider (banner, turns, tool round-trip, session id) per provider.
 * ORACLE: pane output + pane status read back over the API, not the replayer.
 *
 * Runs only against a dev daemon started with CODETERM_STUB_AGENT_CMD; the
 * first test probes for the stub banner and skips the file when absent.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient } from "../../helpers/api-client";

const PROVIDERS = ["claude", "codex", "opencode"] as const;

async function paneOutput(api: ApiClient, paneId: string): Promise<string> {
  // 404 while the PTY materializes after spawn is a normal warmup state.
  const res = await api
    .get<{ output?: string } | string>(`/panes/${paneId}/output`)
    .catch(() => "");
  return typeof res === "string" ? res : (res.output ?? JSON.stringify(res));
}

async function spawnStub(api: ApiClient, provider: string) {
  return api.post<{ pane_id: string }>("/agents/spawn", {
    agent: provider,
    title: `pw-stub-${provider}`,
  });
}

let stubActive: boolean | null = null;

test.beforeEach(async ({ api }) => {
  if (stubActive === null) {
    const spawned = await spawnStub(api, "claude").catch(() => null);
    if (!spawned) {
      stubActive = false;
    } else {
      const seen = await pollUntil(
        async () => (await paneOutput(api, spawned.pane_id)).includes("stub replay"),
        20_000,
      ).catch(() => false);
      stubActive = Boolean(seen);
      await api.closePane(spawned.pane_id).catch(() => {});
    }
  }
  test.skip(!stubActive, "dev daemon is not running with CODETERM_STUB_AGENT_CMD");
});

for (const provider of PROVIDERS) {
  test(`@stub ${provider}: spawn boots the replayer and a marker turn round-trips`, async ({
    api,
    paneCleanup,
  }) => {
    test.setTimeout(90_000);
    const spawned = await spawnStub(api, provider);
    paneCleanup.track(spawned.pane_id);

    await pollUntil(
      async () => (await paneOutput(api, spawned.pane_id)).includes("stub replay"),
      30_000,
      250,
      `${provider} stub banner`,
    );

    const marker = `PWSTUB${Date.now()}`;
    await api.post(`/panes/${spawned.pane_id}/send`, { text: `STUB_ECHO ${marker}`, enter: true });
    await pollUntil(
      async () => (await paneOutput(api, spawned.pane_id)).includes(`ack ${marker}`),
      30_000,
      250,
      `${provider} marker ack`,
    );

    const tool = `PWTOOL${Date.now()}`;
    await api.post(`/panes/${spawned.pane_id}/send`, { text: `STUB_TOOL ${tool}`, enter: true });
    const buffer = await pollUntil(
      async () => {
        const out = await paneOutput(api, spawned.pane_id);
        return out.includes(`tool round-trip done ${tool}`) ? out : null;
      },
      30_000,
      250,
      `${provider} tool round-trip`,
    );
    expect(buffer, "tool command echoed through the pipeline").toContain(tool);
  });
}

test("@stub claude: session id materializes via the session_start hook", async ({
  api,
  paneCleanup,
}) => {
  test.setTimeout(90_000);
  const spawned = await spawnStub(api, "claude");
  paneCleanup.track(spawned.pane_id);

  const sid = await pollUntil(
    async () => {
      const panes = await api
        .get<Array<{ id: string; agent_session_id?: string | null }>>("/panes")
        .catch(() => []);
      return panes.find((p) => p.id === spawned.pane_id)?.agent_session_id || null;
    },
    30_000,
    250,
    "claude stub session id",
  );
  expect(sid, "stub-announced session id is non-empty").toBeTruthy();
});

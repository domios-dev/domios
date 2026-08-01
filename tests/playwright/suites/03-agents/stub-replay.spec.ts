/**
 * WHAT: stub agent replayer drives the REAL spawn/detection/parse pipeline.
 * WHY: golden agent-flow tests convert to this transport; it must behave like
 *      a live provider (banner, turns, tool round-trip, session id) per provider.
 * ORACLE: tab output + tab status read back over the API, not the replayer.
 *
 * Runs only against a dev daemon started with CODETERM_STUB_AGENT_CMD; the
 * first test probes for the stub banner and skips the file when absent.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient } from "../../helpers/api-client";
import path from "node:path";

const PROVIDERS = ["claude", "codex", "opencode"] as const;

async function tabOutput(api: ApiClient, tabId: string): Promise<string> {
  // 404 while the PTY materializes after spawn is a normal warmup state.
  const res = await api
    .get<{ output?: string } | string>(`/tabs/${tabId}/output`)
    .catch(() => "");
  return typeof res === "string" ? res : (res.output ?? JSON.stringify(res));
}

async function spawnStub(api: ApiClient, provider: string) {
  return api.post<{ tab_id: string }>("/agents/spawn", {
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
        async () => (await tabOutput(api, spawned.tab_id)).includes("stub replay"),
        30_000,
        500,
        "stub gate probe",
      ).catch(() => false);
      stubActive = Boolean(seen);
      await api.closeTab(spawned.tab_id).catch(() => {});
    }
  }
  test.skip(!stubActive, "dev daemon is not running with CODETERM_STUB_AGENT_CMD");
});

for (const provider of PROVIDERS) {
  test(`@stub ${provider}: spawn boots the replayer and a marker turn round-trips`, async ({
    api,
    tabCleanup,
  }) => {
    test.setTimeout(90_000);
    const spawned = await spawnStub(api, provider);
    tabCleanup.track(spawned.tab_id);

    await pollUntil(
      async () => (await tabOutput(api, spawned.tab_id)).includes("stub replay"),
      30_000,
      250,
      `${provider} stub banner`,
    );

    const marker = `PWSTUB${Date.now()}`;
    const capture = path.join(process.cwd(), "scratchpad", `${provider}-${marker}.cast`);
    await api.post(`/tabs/${spawned.tab_id}/capture/start`, { out: capture });
    try {
      await api.post(`/tabs/${spawned.tab_id}/send`, { text: `STUB_ECHO ${marker}`, enter: true });
      await pollUntil(
        async () => (await tabOutput(api, spawned.tab_id)).includes(`ack ${marker}`),
        30_000,
        250,
        `${provider} marker ack`,
      );
    } finally {
      await api.post(`/tabs/${spawned.tab_id}/capture/stop`).catch(() => {});
    }

    const tool = `PWTOOL${Date.now()}`;
    await api.post(`/tabs/${spawned.tab_id}/send`, { text: `STUB_TOOL ${tool}`, enter: true });
    const buffer = await pollUntil(
      async () => {
        const out = await tabOutput(api, spawned.tab_id);
        return out.includes(`tool round-trip done ${tool}`) ? out : null;
      },
      30_000,
      250,
      `${provider} tool round-trip`,
    );
    expect(buffer, "tool command echoed through the pipeline").toContain(tool);
  });
}

for (const provider of PROVIDERS) {
  test(`@stub ${provider}: detection binds provider identity and session id`, async ({
    api,
    tabCleanup,
  }) => {
    test.setTimeout(90_000);
    const spawned = await spawnStub(api, provider);
    tabCleanup.track(spawned.tab_id);

    // Same trusted-binding path a real CLI uses before its session file lands:
    // the replayer reports through the nonce-verified session_start hook.
    const tab = await pollUntil(
      async () => {
        const tabs = await api
          .get<Array<{ id: string; agent_type?: string | null; agent_session_id?: string | null }>>(
            "/tabs",
          )
          .catch(() => []);
        const p = tabs.find((x) => x.id === spawned.tab_id);
        return p?.agent_session_id ? p : null;
      },
      30_000,
      250,
      `${provider} stub session id`,
    );
    expect(tab?.agent_type, "tab keeps the real provider identity").toBe(provider);
    expect(tab?.agent_session_id, "session id bound via session_start hook").toBeTruthy();
  });
}

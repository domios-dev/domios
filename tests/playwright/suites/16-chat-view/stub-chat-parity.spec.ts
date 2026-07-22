/**
 * WHAT: ChatView/tailer parity for the stub agent transport.
 * WHY: converted golden tests assert chat messages; the pane chat pipeline
 *      reads the provider SESSION FILE via the tailer, so the stub must
 *      materialize turns in the provider store exactly like a real CLI.
 * ORACLE: GET /panes/{id}/chat (tailer output), same endpoint real chat
 *      tests poll — not the replayer's stdout.
 *
 * Runs only against a dev daemon started with CODETERM_STUB_AGENT_CMD.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient } from "../../helpers/api-client";

type ChatMsg = { type?: string; content?: string };

async function paneOutput(api: ApiClient, paneId: string): Promise<string> {
  const res = await api
    .get<{ output?: string } | string>(`/panes/${paneId}/output`)
    .catch(() => "");
  return typeof res === "string" ? res : (res.output ?? "");
}

async function chatMessages(api: ApiClient, paneId: string): Promise<ChatMsg[]> {
  const res = await api
    .get<{ messages: ChatMsg[] }>(`/panes/${paneId}/chat`)
    .catch(() => ({ messages: [] as ChatMsg[] }));
  return res.messages ?? [];
}

let stubActive: boolean | null = null;

test.beforeEach(async ({ api }) => {
  if (stubActive === null) {
    const spawned = await api
      .post<{ pane_id: string }>("/agents/spawn", { agent: "claude", title: "pw-stub-chat-probe" })
      .catch(() => null);
    if (!spawned) {
      stubActive = false;
    } else {
      const seen = await pollUntil(
        async () => (await paneOutput(api, spawned.pane_id)).includes("stub replay"),
        30_000,
        500,
        "stub gate probe",
      ).catch(() => false);
      stubActive = Boolean(seen);
      await api.closePane(spawned.pane_id).catch(() => {});
    }
  }
  test.skip(!stubActive, "dev daemon is not running with CODETERM_STUB_AGENT_CMD");
});

test("@stub claude: scripted turn lands in pane chat via the session tailer", async ({
  api,
  paneCleanup,
}) => {
  test.setTimeout(90_000);
  const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
    agent: "claude",
    title: "pw-stub-chat",
  });
  paneCleanup.track(spawned.pane_id);

  await pollUntil(
    async () => (await paneOutput(api, spawned.pane_id)).includes("stub replay"),
    30_000,
    250,
    "stub banner",
  );

  const marker = `PWCHAT${Date.now()}`;
  await api.post(`/panes/${spawned.pane_id}/send`, { text: `STUB_ECHO ${marker}`, enter: true });

  const msg = await pollUntil(
    async () =>
      (await chatMessages(api, spawned.pane_id)).find((m) =>
        (m.content ?? "").includes(`ack ${marker}`),
      ) ?? null,
    45_000,
    500,
    "assistant turn visible in pane chat (tailer)",
  );
  expect(msg?.type, "tailer classifies the turn as assistant").toBe("assistant");
});

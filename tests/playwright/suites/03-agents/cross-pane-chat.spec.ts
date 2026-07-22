/**
 * WHAT: cross pane chat covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 03-agents.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type SpawnAgent } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";
import { pollUntil } from "../../helpers/poll";
import { isStubTransportActive } from "../../helpers/stub-gate";

interface ChatMsg {
  role?: string;
  type?: string;
  content?: string;
}

async function chatContains(api: ApiClient, paneId: string, needle: string): Promise<ChatMsg | undefined> {
  // 400 while the session binds after spawn is a normal warmup state.
  const res = await api
    .get<{ messages: ChatMsg[] }>(`/panes/${paneId}/chat`)
    .catch(() => ({ messages: [] as ChatMsg[] }));
  return res.messages.find((m) => (m.content ?? "").includes(needle));
}

async function waitChatContains(api: ApiClient, paneId: string, needle: string, timeout = 60_000): Promise<ChatMsg> {
  const msg = await pollUntil(
    () => chatContains(api, paneId, needle),
    timeout,
    500,
    `pane ${paneId} chat receives "${needle}"`,
  );
  if (!msg) throw new Error(`pane ${paneId} chat never received "${needle}"`);
  return msg;
}

async function spawnChatAgent(api: ApiClient, spawnAgent: SpawnAgent, title: string): Promise<string> {
  const spawned = await spawnAgent({
    agent: "claude",
    model: "sonnet",
    title,
    skip_permissions: true,
    activate: true,
    task: "STUB_ECHO READY",
  });
  // Flip the pane into chat UI (API toggle broadcasts pane-chat-view to clients).
  await api.post(`/panes/${spawned.pane_id}/chat-view`, { enabled: true });
  return spawned.pane_id;
}

test("@golden two chat panes exchange a cross-pane message, origin rendered", async ({ api, ui, spawnAgent }) => {
  test.setTimeout(150_000);
  test.skip(!(await isStubTransportActive(api)), "dev daemon is not running with CODETERM_STUB_AGENT_CMD");

  const stamp = Date.now();
  const aTitle = `pw-A-${stamp}`;
  const bTitle = `pw-B-${stamp}`;
  const aToB = `PING-${stamp}`;
  const bToA = `PONG-${stamp}`;
  const [paneA, paneB] = await Promise.all([
    spawnChatAgent(api, spawnAgent, aTitle),
    spawnChatAgent(api, spawnAgent, bTitle),
  ]);

  // Both panes must report chat-view on — the "both in chat UI" guarantee.
  for (const id of [paneA, paneB]) {
    const pane = (await api.panes()).find((p) => p.id === id) as
      | { chat_view?: boolean }
      | undefined;
    expect(pane?.chat_view, `pane ${id} not in chat view`).toBe(true);
  }

  // send lands on an input-ready CLI rather than a still-spawning PTY.
  await Promise.all([waitChatContains(api, paneA, "READY"), waitChatContains(api, paneB, "READY")]);

  // Real cross-pane sends in BOTH directions, server-prefixed via source_pane.
  await api.post(`/panes/${paneB}/send`, { text: aToB, source_pane: paneA, enter: true });
  await api.post(`/panes/${paneA}/send`, { text: bToA, source_pane: paneB, enter: true });

  // the CodeTerm wire prefix — that is the cross-pane return-address.
  const onB = await waitChatContains(api, paneB, aToB);
  const onA = await waitChatContains(api, paneA, bToA);
  expect(onB.content, "recipient B lost the cross-pane wire prefix").toContain("-=-codeterm:pane");
  expect(onA.content, "recipient A lost the cross-pane wire prefix").toContain("-=-codeterm:pane");

  const tabIdOf = async (paneId: string): Promise<string> => {
    const pane = (await api.panes()).find((p) => p.id === paneId) as
      | { tab_id?: string }
      | undefined;
    const tabId = pane?.tab_id;
    if (!tabId) throw new Error(`pane ${paneId} has no tab_id`);
    return tabId;
  };
  for (const { id, marker } of [
    { id: paneB, marker: aToB },
    { id: paneA, marker: bToA },
  ]) {
    const tabId = await tabIdOf(id);
    await ui.page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first().click();
    await ui.page.locator('[data-testid="chat-composer"]').first().waitFor({
      state: "visible",
      timeout: 30_000,
    });
    const msg = ui.page
      .locator('[data-testid="chat-message"][data-message-role="user"]')
      .filter({ hasText: marker })
      .first();
    await msg.waitFor({ state: "visible", timeout: 60_000 });
    await expect(msg, "origin header missing — marker not parsed into a 'from' chip").toContainText(
      "from",
    );
    await expect(msg, "raw wire marker leaked into the rendered chat").not.toContainText(
      "-=-codeterm:",
    );
  }
});

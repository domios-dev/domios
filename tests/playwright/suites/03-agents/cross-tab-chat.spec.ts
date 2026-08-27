/**
 * WHAT: cross tab chat covers its Playwright acceptance contract.
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

async function chatContains(api: ApiClient, tabId: string, needle: string): Promise<ChatMsg | undefined> {
  // 400 while the session binds after spawn is a normal warmup state.
  const res = await api
    .get<{ messages: ChatMsg[] }>(`/tabs/${tabId}/chat`)
    .catch(() => ({ messages: [] as ChatMsg[] }));
  return res.messages.find((m) => (m.content ?? "").includes(needle));
}

async function waitChatContains(api: ApiClient, tabId: string, needle: string, timeout = 60_000): Promise<ChatMsg> {
  const msg = await pollUntil(
    () => chatContains(api, tabId, needle),
    timeout,
    500,
    `tab ${tabId} chat receives "${needle}"`,
  );
  if (!msg) throw new Error(`tab ${tabId} chat never received "${needle}"`);
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
  // Flip the tab into chat UI (API toggle broadcasts tab-chat-view to clients).
  await api.post(`/tabs/${spawned.tab_id}/chat-view`, { enabled: true });
  return spawned.tab_id;
}

test("@golden two chat tabs exchange a cross-tab message, origin rendered", async ({ api, ui, spawnAgent }) => {
  test.setTimeout(150_000);
  test.skip(!(await isStubTransportActive(api)), "dev daemon is not running with CODETERM_STUB_AGENT_CMD");

  const stamp = Date.now();
  const aTitle = `pw-A-${stamp}`;
  const bTitle = `pw-B-${stamp}`;
  const aToB = `PING-${stamp}`;
  const bToA = `PONG-${stamp}`;
  const [tabA, tabB] = await Promise.all([
    spawnChatAgent(api, spawnAgent, aTitle),
    spawnChatAgent(api, spawnAgent, bTitle),
  ]);

  // Both tabs must report chat-view on — the "both in chat UI" guarantee.
  for (const id of [tabA, tabB]) {
    const tab = (await api.tabs()).find((p) => p.id === id) as
      | { chat_view?: boolean }
      | undefined;
    expect(tab?.chat_view, `tab ${id} not in chat view`).toBe(true);
  }

  // send lands on an input-ready CLI rather than a still-spawning PTY.
  await Promise.all([waitChatContains(api, tabA, "READY"), waitChatContains(api, tabB, "READY")]);

  // Real cross-tab sends in BOTH directions, server-prefixed via source_tab.
  await api.post(`/tabs/${tabB}/send`, { text: aToB, source_tab: tabA, enter: true });
  await api.post(`/tabs/${tabA}/send`, { text: bToA, source_tab: tabB, enter: true });

  // the CodeTerm wire prefix — that is the cross-tab return-address.
  const onB = await waitChatContains(api, tabB, aToB);
  const onA = await waitChatContains(api, tabA, bToA);
  expect(onB.content, "recipient B lost the cross-tab wire prefix").toContain("-=-codeterm:tab");
  expect(onA.content, "recipient A lost the cross-tab wire prefix").toContain("-=-codeterm:tab");

  // The sidebar row keys off the tab's own id; panes no longer exist. Kept as a
  // lookup so a vanished tab fails here instead of as a selector timeout.
  const tabIdOf = async (wanted: string): Promise<string> => {
    const tab = (await api.tabs()).find((t) => t.id === wanted);
    if (!tab) throw new Error(`tab ${wanted} not found in /tabs`);
    return tab.id;
  };
  for (const { id, marker } of [
    { id: tabB, marker: aToB },
    { id: tabA, marker: bToA },
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

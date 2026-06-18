/**
 * Cross-pane chat send — two live agent panes in chat UI exchange a message.
 *
 * This guards the full cross-pane pipeline the user relies on:
 *   sender pane → POST /send {source_pane} → server builds the
 *   `-=-codeterm:pane label=… pane=…-=-` prefix → injects into the
 *   recipient agent's PTY → the CLI logs the user turn to its JSONL →
 *   the chat UI tails it, parses the marker, and renders an origin header
 *   ("from <sender>") instead of the raw wire marker.
 *
 * Strategy (matches 03-agents): NO mocks. Real claude agents, real PTYs.
 * The ASSERTIONS key off the deterministic injected user turn (our exact
 * marker), never model output — so the only LLM dependency is that the
 * agents boot and accept input, not that they say anything in particular.
 */
import { expect, test, type SpawnAgent } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";

interface ChatMsg {
  role?: string;
  type?: string;
  content?: string;
}

async function chatContains(api: ApiClient, paneId: string, needle: string): Promise<ChatMsg | undefined> {
  const res = await api.get<{ messages: ChatMsg[] }>(`/panes/${paneId}/chat`);
  return res.messages.find((m) => (m.content ?? "").includes(needle));
}

async function waitChatContains(api: ApiClient, paneId: string, needle: string, timeout = 120_000): Promise<ChatMsg> {
  const deadline = Date.now() + timeout;
  let last: ChatMsg | undefined;
  while (Date.now() < deadline) {
    last = await chatContains(api, paneId, needle);
    if (last) return last;
    await new Promise((r) => setTimeout(r, 2_000));
  }
  throw new Error(`pane ${paneId} chat never received "${needle}"`);
}

async function spawnChatAgent(api: ApiClient, spawnAgent: SpawnAgent, title: string): Promise<string> {
  const spawned = await spawnAgent({
    agent: "claude",
    model: "sonnet",
    title,
    skip_permissions: true,
    activate: true,
    task:
      "You are a passive test fixture. Reply with the single word READY now. " +
      "For any later message, reply with only the word ACK. " +
      "Never run shell commands and never run any codeterm command.",
  });
  // Flip the pane into chat UI (API toggle broadcasts pane-chat-view to clients).
  await api.post(`/panes/${spawned.pane_id}/chat-view`, { enabled: true });
  return spawned.pane_id;
}

test("@golden two chat panes exchange a cross-pane message, origin rendered", async ({ api, ui, spawnAgent }) => {
  // Two agent boots + a turn each. Bounded for real model + CLI latency.
  test.setTimeout(420_000);

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

  // Wait until each agent has booted and logged a turn (READY) so the next
  // send lands on an input-ready CLI rather than a still-spawning PTY.
  await Promise.all([waitChatContains(api, paneA, "READY"), waitChatContains(api, paneB, "READY")]);

  // Real cross-pane sends in BOTH directions, server-prefixed via source_pane.
  await api.post(`/panes/${paneB}/send`, { text: aToB, source_pane: paneA, enter: true });
  await api.post(`/panes/${paneA}/send`, { text: bToA, source_pane: paneB, enter: true });

  // The injected user turns must land in each recipient's transcript carrying
  // the CodeTerm wire prefix — that is the cross-pane return-address.
  const onB = await waitChatContains(api, paneB, aToB);
  const onA = await waitChatContains(api, paneA, bToA);
  expect(onB.content, "recipient B lost the cross-pane wire prefix").toContain("-=-codeterm:pane");
  expect(onA.content, "recipient A lost the cross-pane wire prefix").toContain("-=-codeterm:pane");

  // UI side: switch to each chat tab and assert the marker renders inside a
  // chat message WITH an origin header ("from …") and WITHOUT the raw wire
  // marker leaking — proving the client-side marker parse ran. Focus alone
  // doesn't change the visible tab; click the tab-item (resolved by tab id,
  // robust against agent auto-rename) to actually mount that pane's chat.
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
    // Target the injected USER turn specifically — a reacting agent may emit
    // its own tool_call cards that also echo the marker, so role-filter to the
    // deterministic incoming message that carries the origin header.
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

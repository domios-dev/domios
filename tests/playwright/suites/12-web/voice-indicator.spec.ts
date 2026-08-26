/**
 * WHAT: voice indicator covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 12-web.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";
import { isStubTransportActive } from "../../helpers/stub-gate";

const WEB_URL = `${API_BASE}/`;

// composer's upload path runs without a real microphone.
const FAKE_MEDIA = `
  navigator.mediaDevices = navigator.mediaDevices || {};
  navigator.mediaDevices.getUserMedia = async () => ({ getTracks: () => [{ stop() {} }] });
  if (navigator.permissions) navigator.permissions.query = async () => ({ state: 'granted' });
  class FakeRecorder {
    constructor() { this.state='inactive'; this.mimeType='audio/ogg'; this.ondataavailable=null; this.onstop=null; this.onerror=null; }
    start() { this.state='recording'; }
    stop() {
      this.state='inactive';
      const blob = new Blob([new Uint8Array(8192)], { type: this.mimeType });
      this.ondataavailable && this.ondataavailable({ data: blob });
      this.onstop && this.onstop();
    }
    static isTypeSupported() { return true; }
  }
  window.MediaRecorder = FakeRecorder;
`;

async function holdVoice(page: Page): Promise<void> {
  const btn = page.locator('[data-testid="composer-voice-button"]').filter({ visible: true }).first();
  const box = await btn.boundingBox();
  if (!box) throw new Error("voice button has no box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await expect(page.locator('[data-testid="composer-voice-live-waveform"]')).toBeVisible({
    timeout: 5_000,
  });
  await page.mouse.up();
}

test("@stub voice note: transcribing placeholder morphs into transcript, and failure shows an honest error", async ({ api, webBrowser, tabCleanup, spawnAgent, mediaMocks }) => {
  test.setTimeout(180_000);
  // The transcription pipeline, not the provider, is under test — spawn through
  // the stub transport so the assertion never depends on provider credit.
  test.skip(
    !(await isStubTransportActive(api)),
    "dev daemon is not running with CODETERM_STUB_AGENT_CMD",
  );
  const ctx = await webBrowser.newContext({ viewport: { width: 375, height: 812 } });
  const page = await ctx.newPage();
  await mediaMocks.installVoiceRecorder(page, FAKE_MEDIA);

  // Mock the upload (fast) — transcribe is (re)routed per phase below.
  await mediaMocks.mockUpload(page);

  // remains) → exactly one chat composer once we spawn below.
  await tabCleanup.closeUnprotectedTabs();

  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });

  // Mobile viewport defaults the spawned agent tab to chat mode → the mic renders.
  const inbox = await tabCleanup.createTab({ title: "pw-voice-inbox" });
  let agentTabId: string | undefined;
  try {
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-voice-agent",
      parent_tab_id: inbox.id,
      report_target_tab_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: "Wait silently.",
    });
    agentTabId = spawned.tab_id;
    await page.locator('[data-testid="composer-voice-button"]').filter({ visible: true }).first().waitFor({ timeout: 60_000 });

    await mediaMocks.mockTranscription(page, { line: "hello from the transcript", delayMs: 2500 });
    await holdVoice(page);
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "Transcribing voice note" }),
    ).toBeVisible({ timeout: 2000 });
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "hello from the transcript" }),
    ).toBeVisible({ timeout: 8000 });
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "Transcribing voice note" }),
    ).toHaveCount(0);

    await page.unroute("**/api/transcribe-voice");
    await mediaMocks.mockTranscription(page, { status: 500, body: "transcribe daemon down" });
    await holdVoice(page);
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "transcription failed" }),
    ).toBeVisible({ timeout: 8000 });
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
    await ctx.close();
  }
});

/**
 * 12-web / voice-indicator — a voice note shows an immediate "transcribing…"
 * bubble that morphs into the transcript, and an honest error bubble when
 * transcription fails — instead of a blank gap while the (possibly cold)
 * Whisper daemon works. See issue #158.
 *
 * MediaRecorder + getUserMedia are stubbed (deterministic >MIN_VOICE_BYTES blob,
 * no real mic), and /api/upload + /api/transcribe-voice are mocked so we control
 * the transcription latency and the success/failure outcome. Both phases run in
 * ONE spawned chat pane so there is exactly one composer in the DOM.
 */
import { expect, test, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

const WEB_URL = `${API_BASE}/web/`;

// Deterministic recorder: stop() emits one >5KB chunk then fires onstop, so the
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
  await page.waitForTimeout(400);
  await page.mouse.up();
}

test("@golden voice note: transcribing placeholder morphs into transcript, and failure shows an honest error", async ({ api, webBrowser, paneCleanup, spawnAgent, mediaMocks }) => {
  test.setTimeout(180_000);
  const ctx = await webBrowser.newContext({ viewport: { width: 375, height: 812 } });
  const page = await ctx.newPage();
  await mediaMocks.installVoiceRecorder(page, FAKE_MEDIA);

  // Mock the upload (fast) — transcribe is (re)routed per phase below.
  await mediaMocks.mockUpload(page);

  // Close panes left by earlier runs (keep workspaces so an active workspace
  // remains) → exactly one chat composer once we spawn below.
  await paneCleanup.closeUnprotectedPanes();

  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });

  // Mobile viewport defaults the spawned agent pane to chat mode → the mic renders.
  const inbox = await paneCleanup.createPane({ title: "pw-voice-inbox" });
  let agentPaneId: string | undefined;
  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-voice-agent",
      parent_pane_id: inbox.id,
      report_target_pane_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: "Wait silently.",
    });
    agentPaneId = spawned.pane_id;
    await page.locator('[data-testid="composer-voice-button"]').filter({ visible: true }).first().waitFor({ timeout: 60_000 });

    // ── Phase 1: deliberate 2.5s latency (cold Whisper) → placeholder → transcript ──
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

    // ── Phase 2: daemon down (500) → placeholder → honest error, nothing sent ──
    await page.unroute("**/api/transcribe-voice");
    await mediaMocks.mockTranscription(page, { status: 500, body: "transcribe daemon down" });
    await holdVoice(page);
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "transcription failed" }),
    ).toBeVisible({ timeout: 8000 });
  } finally {
    if (agentPaneId) paneCleanup.track(agentPaneId);
    await ctx.close();
  }
});

/**
 * 12-web / voice-warm-mic — the mic stream is acquired ONCE and reused across
 * recordings (warm), not re-acquired per record. Guards the iOS-PWA re-prompt
 * fix in useVoiceRecorder.ts: iOS standalone re-prompts on every getUserMedia,
 * so two consecutive recordings must trigger exactly ONE getUserMedia call.
 */
import { expect, test, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

const WEB_URL = `${API_BASE}/web/`;

// Counts getUserMedia calls on window.__gumCalls; returns a stream whose audio
// track stays "live" so acquireWarmStream() reuses it on the second record.
const FAKE_MEDIA = `
  window.__gumCalls = 0;
  const liveTrack = { stop() {}, readyState: 'live', kind: 'audio' };
  const fakeStream = { getTracks: () => [liveTrack], getAudioTracks: () => [liveTrack] };
  navigator.mediaDevices = navigator.mediaDevices || {};
  navigator.mediaDevices.getUserMedia = async () => { window.__gumCalls++; return fakeStream; };
  if (navigator.permissions) navigator.permissions.query = async () => ({ state: 'granted' });
  class FakeRecorder {
    constructor() { this.state='inactive'; this.mimeType='audio/ogg'; this.ondataavailable=null; this.onstop=null; this.onerror=null; }
    start() { this.state='recording'; }
    stop() {
      this.state='inactive';
      this.ondataavailable && this.ondataavailable({ data: new Blob([new Uint8Array(8192)], { type: this.mimeType }) });
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

test("@golden mic stream is warm: two recordings → getUserMedia called exactly once", async ({ api, webBrowser, paneCleanup, spawnAgent, mediaMocks }) => {
  test.setTimeout(180_000);
  const ctx = await webBrowser.newContext({ viewport: { width: 375, height: 812 } });
  const page = await ctx.newPage();
  await mediaMocks.installVoiceRecorder(page, FAKE_MEDIA);
  await mediaMocks.mockUpload(page);
  await mediaMocks.mockTranscription(page, { line: "warm mic transcript" });

  // Close stale panes so exactly one chat composer mounts.
  await paneCleanup.closeUnprotectedPanes();
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });

  const inbox = await paneCleanup.createPane({ title: "pw-warmmic-inbox" });
  let agentPaneId: string | undefined;
  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-warmmic-agent",
      parent_pane_id: inbox.id,
      report_target_pane_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: "Wait silently.",
    });
    agentPaneId = spawned.pane_id;
    await page.locator('[data-testid="composer-voice-button"]').filter({ visible: true }).first().waitFor({ timeout: 60_000 });

    // First recording — acquires the stream (getUserMedia #1).
    await holdVoice(page);
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "warm mic transcript" }),
    ).toBeVisible({ timeout: 8000 });
    expect(await page.evaluate(() => (window as unknown as { __gumCalls: number }).__gumCalls)).toBe(1);

    // Second recording — must REUSE the warm stream, no new getUserMedia.
    await holdVoice(page);
    await expect(
      page.locator('[data-testid="chat-message"]', { hasText: "warm mic transcript" }),
    ).toHaveCount(2, { timeout: 8_000 });
    expect(await page.evaluate(() => (window as unknown as { __gumCalls: number }).__gumCalls)).toBe(1);
  } finally {
    if (agentPaneId) paneCleanup.track(agentPaneId);
    await ctx.close();
  }
});

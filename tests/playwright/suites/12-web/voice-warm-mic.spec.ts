/**
 * 12-web / voice-warm-mic — the mic stream is acquired ONCE and reused across
 * recordings (warm), not re-acquired per record. Guards the iOS-PWA re-prompt
 * fix in useVoiceRecorder.ts: iOS standalone re-prompts on every getUserMedia,
 * so two consecutive recordings must trigger exactly ONE getUserMedia call.
 */
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { ApiClient, readDevToken } from "../../helpers/api-client";
import { API_BASE } from "../../helpers/dev-console";

const WEB_URL = `${API_BASE}/web/`;
const api = new ApiClient();
let browser: Browser;

test.beforeAll(async () => {
  browser = await chromium.launch({ channel: "msedge" });
});
test.afterAll(async () => {
  await browser.close();
});

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
  await page.waitForTimeout(400);
  await page.mouse.up();
}

test("@golden mic stream is warm: two recordings → getUserMedia called exactly once", async () => {
  test.setTimeout(180_000);
  const ctx = await browser.newContext({ viewport: { width: 375, height: 812 } });
  const page = await ctx.newPage();
  await page.addInitScript(FAKE_MEDIA);
  await page.route("**/api/upload", (r) =>
    r.fulfill({ contentType: "application/json", body: JSON.stringify({ path: "/tmp/pw-voice.ogg", size: 8192 }) }),
  );
  await page.route("**/api/transcribe-voice", (r) =>
    r.fulfill({ contentType: "application/json", body: JSON.stringify({ line: "warm mic transcript" }) }),
  );

  // Close stale panes so exactly one chat composer mounts.
  for (const pane of await api.panes()) {
    if (!pane.protected) await api.closePane(pane.id).catch(() => {});
  }
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });

  const inbox = (await api.createPane({ title: "pw-warmmic-inbox" })) as { id: string };
  let agentPaneId: string | undefined;
  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
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
    await page.waitForTimeout(1500);
    expect(await page.evaluate(() => (window as unknown as { __gumCalls: number }).__gumCalls)).toBe(1);
  } finally {
    if (agentPaneId) await api.closePane(agentPaneId).catch(() => {});
    await api.closePane(inbox.id).catch(() => {});
    await ctx.close();
  }
});

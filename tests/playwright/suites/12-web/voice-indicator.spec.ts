/**
 * 12-web / voice-indicator — a voice note shows an immediate "transcribing…"
 * bubble that morphs into the transcript (or an error) instead of a blank gap
 * while the (possibly cold) Whisper daemon works. See issue #158.
 *
 * MediaRecorder + getUserMedia are stubbed (deterministic >MIN_VOICE_BYTES blob,
 * no real mic), and /api/upload + /api/transcribe-voice are mocked so we control
 * the transcription latency and the success/failure outcome.
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

async function bootChatPane(
  page: Page,
): Promise<{ paneId: string; inboxId: string }> {
  await page.addInitScript(FAKE_MEDIA);
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first().waitFor({ timeout: 20_000 });
  // Mobile viewport defaults agent panes to chat mode → ComposerActions (mic) renders.
  const inbox = (await api.createPane({ title: "pw-voice-inbox" })) as { id: string };
  await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
  const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
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
  await page.locator('[data-testid="composer-voice-button"]').waitFor({ timeout: 60_000 });
  return { paneId: spawned.pane_id, inboxId: inbox.id };
}

async function holdVoice(page: Page): Promise<void> {
  const btn = page.locator('[data-testid="composer-voice-button"]');
  const box = await btn.boundingBox();
  if (!box) throw new Error("voice button has no box");
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.waitForTimeout(400);
  await page.mouse.up();
}

test("@golden voice note shows transcribing placeholder, then morphs into the transcript", async () => {
  const page = await browser.newContext({ viewport: { width: 375, height: 812 } }).then((c) => c.newPage());
  await page.route("**/api/upload", (r) =>
    r.fulfill({ contentType: "application/json", body: JSON.stringify({ path: "/tmp/pw-voice.ogg", size: 8192 }) }),
  );
  // Deliberate 2.5s latency simulates a cold Whisper daemon.
  await page.route("**/api/transcribe-voice", async (r) => {
    await new Promise((res) => setTimeout(res, 2500));
    await r.fulfill({ contentType: "application/json", body: JSON.stringify({ line: "hello from the transcript" }) });
  });

  let ids: { paneId: string; inboxId: string } | undefined;
  try {
    ids = await bootChatPane(page);
    await holdVoice(page);
    // Placeholder appears immediately (well within the 2.5s transcribe window).
    await expect(page.locator('[data-testid="chat-message"]', { hasText: "Transcribing voice note" })).toBeVisible({ timeout: 2000 });
    // Then it morphs into the transcript in place.
    await expect(page.locator('[data-testid="chat-message"]', { hasText: "hello from the transcript" })).toBeVisible({ timeout: 8000 });
    await expect(page.locator('[data-testid="chat-message"]', { hasText: "Transcribing voice note" })).toHaveCount(0);
  } finally {
    if (ids?.paneId) await api.closePane(ids.paneId).catch(() => {});
    if (ids?.inboxId) await api.closePane(ids.inboxId).catch(() => {});
    await page.context().close();
  }
});

test("@golden voice transcription failure surfaces an honest error placeholder", async () => {
  const page = await browser.newContext({ viewport: { width: 375, height: 812 } }).then((c) => c.newPage());
  await page.route("**/api/upload", (r) =>
    r.fulfill({ contentType: "application/json", body: JSON.stringify({ path: "/tmp/pw-voice.ogg", size: 8192 }) }),
  );
  // Daemon down → 500 → transcribe_voice_attachment throws → error placeholder.
  await page.route("**/api/transcribe-voice", (r) => r.fulfill({ status: 500, body: "transcribe daemon down" }));

  let ids: { paneId: string; inboxId: string } | undefined;
  try {
    ids = await bootChatPane(page);
    await holdVoice(page);
    await expect(page.locator('[data-testid="chat-message"]', { hasText: "transcription failed" })).toBeVisible({ timeout: 8000 });
  } finally {
    if (ids?.paneId) await api.closePane(ids.paneId).catch(() => {});
    if (ids?.inboxId) await api.closePane(ids.inboxId).catch(() => {});
    await page.context().close();
  }
});

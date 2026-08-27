/**
 * WHAT: cursor new rebinds session covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 16-chat-view.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import crypto from "crypto";
import { mkdirSync, realpathSync, rmSync, writeFileSync } from "fs";
import os from "os";
import path from "path";
import { CURSOR_AGENT_BIN_ENV, discoverCursorAgent } from "../../helpers/agent-binary";
import { expect, test } from "../../helpers/fixtures";

// Discovered, not hardcoded: a machine-specific path skipped this spec forever
// on every other machine, reporting success while asserting nothing.
const CURSOR_AGENT_BIN = discoverCursorAgent();
const REPLY_1 = "pw-cursor-reply-1";
const REPLY_2 = "pw-cursor-reply-2";

/** Cursor hashes the native cwd (forward slashes on macOS/Linux). */
function md5(s: string): string {
  return crypto.createHash("md5").update(s).digest("hex");
}

/** cursor's project slug: strip leading separators, replace non-alnum with '-'. */
function cursorSlug(cwd: string): string {
  return cwd.replace(/^\/+/, "").replace(/[^a-zA-Z0-9]/g, "-");
}

/** ~/.cursor/chats/<md5(realCwd)>/<sessionId>/meta.json */
function cursorChatsDir(realCwd: string): string {
  return path.join(os.homedir(), ".cursor", "chats", md5(realCwd));
}

/**
 * ~/.cursor/projects/<slug>/agent-transcripts/<sessionId>/<sessionId>.jsonl
 * Uses the cursor slug derived from realCwd to match cursor-agent's pwd -P path.
 */
function jsonlPath(realCwd: string, sessionId: string): string {
  const slug = cursorSlug(realCwd);
  return path.join(
    os.homedir(),
    ".cursor",
    "projects",
    slug,
    "agent-transcripts",
    sessionId,
    `${sessionId}.jsonl`,
  );
}

/** JSONL line that parseSessionDelta renders as an assistant text block. */
function assistantJsonl(text: string): string {
  return (
    JSON.stringify({
      role: "assistant",
      message: { content: [{ type: "text", text }] },
    }) + "\n"
  );
}

test(
  "@live cursor /new re-binds chat to the new session",
  async ({ api, ui, tabCleanup }) => {
    test.skip(
      CURSOR_AGENT_BIN === null,
      `cursor-agent not found on PATH or in a known install dir; set ${CURSOR_AGENT_BIN_ENV} to point at it`,
    );

    const nowMs = Date.now();
    const rawTestCwd = path.join(os.tmpdir(), `codeterm-cursor-pw-${nowMs}`);
    mkdirSync(rawTestCwd, { recursive: true });
    const testCwd = realpathSync(rawTestCwd); // /private/var/folders/... on macOS

    const chatsDir = cursorChatsDir(testCwd);
    const slug = cursorSlug(testCwd);

    // Pre-generate session-2 uuid (simulates the new session cursor creates after /new).
    const uuid2 = crypto.randomUUID();

    let session1: string | null = null;

    await ui.reconnect();

    const spawned = await api.post<{ tab_id: string; tab_id?: string }>("/agents/spawn", {
      agent: "cursor",
      cwd: testCwd,
      title: `pw-cursor-new-${nowMs}`,
      skip_permissions: true,
    });
    tabCleanup.track(spawned.tab_id);
    const tabId = spawned.tab_id;

    const tick = setInterval(async () => {
      await api
        .post(`/tabs/${tabId}/send`, { text: "x", enter: false, raw: true })
        .catch(() => {});
      await api
        .post(`/tabs/${tabId}/send`, {
          actions: [{ type: "key", key: "backspace" }],
          enter: false,
          raw: true,
        })
        .catch(() => {});
    }, 1_000);

    try {
      for (let i = 0; i < 120; i++) {
        // PW_JUSTIFIED_SLEEP: fixed API polling interval within a 30s budget.
        await ui.page.waitForTimeout(250);
        const tabs = await api.get<{ id: string; agent_session_id?: string | null }[]>("/tabs");
        const tab = tabs.find((p) => p.id === tabId);
        if (tab?.agent_session_id) {
          session1 = tab.agent_session_id;
          break;
        }
      }

      expect(
        session1,
        [
          "cursor-agent must call the CodeTerm hook and set agent_session_id within 30s.",
          "If this times out: cursor-agent may have exited early or the hook endpoint is unreachable.",
        ].join(" "),
      ).not.toBeNull();

      const jp1 = jsonlPath(testCwd, session1!);
      mkdirSync(path.dirname(jp1), { recursive: true });
      writeFileSync(jp1, assistantJsonl(REPLY_1), "utf8");

      await api.post(`/tabs/${tabId}/chat-view`, { enabled: true });
      await api.focusTab(tabId);
      await ui.page.reload({ waitUntil: "domcontentloaded" });
      await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

      if (spawned.tab_id) {
        const tab = ui.page
          .locator(`[data-testid="tab-item"][data-tab-id="${spawned.tab_id}"]`)
          .first();
        await expect(tab).toBeVisible({ timeout: 30_000 });
        await tab.click();
      }

      const chat = ui.page.locator(
        `[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`,
      );
      await expect(chat).toBeVisible({ timeout: 30_000 });

      // Confirms initial binding works: session-1's reply is visible.
      const msg1 = chat
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: REPLY_1 })
        .first();
      await expect(msg1).toBeVisible({ timeout: 30_000 });

      const session2Dir = path.join(chatsDir, uuid2);
      mkdirSync(session2Dir, { recursive: true });
      writeFileSync(
        path.join(session2Dir, "meta.json"),
        JSON.stringify({ createdAtMs: Date.now(), updatedAtMs: Date.now() }),
        "utf8",
      );

      let reboundSession: string | null = null;
      for (let i = 0; i < 160; i++) {
        // PW_JUSTIFIED_SLEEP: fixed API polling interval within a 40s budget.
        await ui.page.waitForTimeout(250);
        const tabs = await api.get<{ id: string; agent_session_id?: string | null }[]>("/tabs");
        const tab = tabs.find((p) => p.id === tabId);
        if (tab?.agent_session_id && tab.agent_session_id !== session1) {
          reboundSession = tab.agent_session_id;
          break;
        }
      }

      expect(
        reboundSession,
        [
          `agent_session_id must change from session-1 (${session1}) to uuid2 (${uuid2}).`,
          "RED (old code): is_some() guard exits early → stays session1 forever → timeout.",
          "GREEN (fixed): stale check runs → finds uuid2 (only fresh non-excluded candidate)",
          "→ evicts session1 → rebinds to uuid2.",
        ].join(" "),
      ).toBe(uuid2);

      const jp2 = jsonlPath(testCwd, uuid2);
      mkdirSync(path.dirname(jp2), { recursive: true });
      writeFileSync(jp2, assistantJsonl(REPLY_2), "utf8");

      const msg2 = chat
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: REPLY_2 })
        .first();
      await expect(msg2).toBeVisible({ timeout: 30_000 });
    } finally {
      clearInterval(tick);

      // Remove the cursor chats dir for testCwd (contains session-2 dir we created).
      try {
        rmSync(chatsDir, { recursive: true, force: true });
      } catch {
        /* best effort */
      }
      // Remove JSONL transcript dirs we created for both sessions.
      try {
        rmSync(
          path.join(os.homedir(), ".cursor", "projects", slug, "agent-transcripts"),
          { recursive: true, force: true },
        );
      } catch {
        /* best effort */
      }
      // Remove temp testCwd.
      try {
        rmSync(rawTestCwd, { recursive: true, force: true });
      } catch {
        /* best effort */
      }
    }
  },
);

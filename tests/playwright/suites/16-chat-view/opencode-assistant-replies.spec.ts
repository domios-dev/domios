/**
 * WHAT: opencode assistant replies covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 16-chat-view.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { execSync, spawnSync } from "child_process";
import { existsSync } from "fs";
import os from "os";
import path from "path";
import { expect, test } from "../../helpers/fixtures";

function opencodeDbPath(): string {
  const xdg = process.env.XDG_DATA_HOME ?? path.join(os.homedir(), ".local", "share");
  return path.join(xdg, "opencode", "opencode.db");
}

function q(s: string): string {
  return s.replace(/'/g, "''");
}

test(
  "@live opencode db-fallback binds session without hook and shows assistant replies",
  async ({ api, ui, tabCleanup }) => {
    const db = opencodeDbPath();
    test.skip(!existsSync(db), "opencode DB not found — opencode not installed on this machine");

    // Need an existing project_id to satisfy the FK constraint.
    let projectId: string;
    try {
      projectId = execSync(`sqlite3 "${db}" "SELECT id FROM project LIMIT 1"`, {
        timeout: 5_000,
      })
        .toString()
        .trim();
    } catch {
      projectId = "";
    }
    test.skip(!projectId, "no opencode project in DB — run opencode at least once first");

    const nowMs = Date.now();
    const sessionId = `ses_pw${nowMs.toString(16).padStart(18, "0")}`;
    const msgId = `msg_pw${nowMs.toString(16).padStart(18, "0")}`;
    const partId = `part_pw${nowMs.toString(16).padStart(18, "0")}`;
    const assistantText = "pw-opencode-db-fallback-reply";

    function sqliteExec(sql: string): void {
      const r = spawnSync("sqlite3", [db], { input: sql, timeout: 5_000, encoding: "utf8" });
      if (r.status !== 0) throw new Error(`sqlite3 failed (${r.status}): ${r.stderr || r.error}`);
    }
    sqliteExec(
      `INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('${q(sessionId)}', '${q(projectId)}', 'pw-test', '${q(process.cwd())}', 'Playwright fallback test', '1', ${nowMs}, ${nowMs});`,
    );
    sqliteExec(
      `INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('${q(msgId)}', '${q(sessionId)}', ${nowMs}, ${nowMs}, '${q(JSON.stringify({ role: "assistant", modelID: "test" }))}');`,
    );
    sqliteExec(
      `INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('${q(partId)}', '${q(msgId)}', '${q(sessionId)}', ${nowMs}, ${nowMs}, '${q(JSON.stringify({ type: "text", text: assistantText }))}');`,
    );

    await ui.reconnect();

    const spawned = await api.post<{ tab_id: string; tab_id?: string }>("/agents/spawn", {
      agent: "opencode",
      args: ["--session", sessionId],
      title: `pw-opencode-fallback-${nowMs}`,
    });
    tabCleanup.track(spawned.tab_id);
    const tabId = spawned.tab_id;

    try {
      let boundSessionId: string | null = null;
      for (let i = 0; i < 120; i++) {
        // PW_JUSTIFIED_SLEEP: fixed API polling interval within a 30s budget.
        await ui.page.waitForTimeout(250);
        const tabs = await api.get<{ id: string; agent_session_id?: string }[]>("/tabs");
        const tab = tabs.find((p) => p.id === tabId);
        if (tab?.agent_session_id) {
          boundSessionId = tab.agent_session_id;
          break;
        }
      }

      expect(
        boundSessionId,
        [
          "DB fallback must bind agent_session_id within 30s of TUI output.",
          "The hook was NOT fired in this test — binding must come from detectSessionId only.",
          "RED (old code): needs=false because has_hooks=true blocked detection → null.",
          "GREEN (fixed): needs=true via uses_structured_chat() → DB query → bound.",
        ].join(" "),
      ).toBe(sessionId);

      // Enable chat-view and verify the pre-inserted assistant reply renders.
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

      // THE KEY ASSERTION: assistant reply from the pre-inserted transcript must appear.
      // Before the fix: session stays null → no transcript poll → no assistant reply.
      const assistantMsg = chat
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: assistantText })
        .first();
      await expect(assistantMsg).toBeVisible({ timeout: 30_000 });
    } finally {
      // Remove test rows from the real opencode DB (CASCADE deletes messages + parts).
      try {
        execSync(
          `sqlite3 "${db}" "DELETE FROM session WHERE id='${q(sessionId)}'"`,
          { timeout: 5_000 },
        );
      } catch {
        /* best effort */
      }
    }
  },
);

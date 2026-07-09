/**
 * Gate: opencode db-fallback detects session and shows ASSISTANT replies without any hook.
 *
 * Bug being fixed:
 *   agent_session_id stayed null for opencode panes when the session-binding hook didn't
 *   fire. `maybe_detect_agent_session` was gated by `!p.has_hooks()` which excluded opencode
 *   (has_hooks=true), so disk/db detection never ran → structured chat never started →
 *   user messages appeared (optimistic) but assistant replies never rendered.
 *
 * Fix:
 *   Gate is now `!p.has_hooks() || p.uses_structured_chat()`. Opencode satisfies the
 *   second clause (sqlite-backed structured chat), so detection runs as a fallback even
 *   when hooks are present. Claude/codex are NOT affected: they have hooks AND
 *   uses_structured_chat()=false → gate stays false → unchanged behaviour.
 *
 * RED (without fix): needs=false for opencode → session stays null → test times out at
 *   the agent_session_id polling step.
 * GREEN (after fix): needs=true → detectSessionId queries opencode.db → finds fresh session
 *   → binds → structured chat starts → assistant replies visible.
 *
 * Scenario:
 *   1. Pre-insert a FRESH session (time_created = now) into opencode.db so it is within
 *      the detection max_age window (~30s for a freshly spawned pane).
 *   2. Spawn opencode with `--session <new-id>`. The hook does NOT fire (test never calls
 *      the hook endpoint). agent_session_id starts null.
 *   3. TUI output arrives as opencode starts. maybe_detect_agent_session runs, calls
 *      detectSessionId → finds the fresh session → sets agent_session_id.
 *   4. Enable chat-view and assert the pre-inserted assistant reply renders.
 *
 * macOS web-mode (no opencode needed for basic CI, skips when DB absent):
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden -g "opencode"
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
  "@golden opencode db-fallback binds session without hook and shows assistant replies",
  async ({ api, ui, paneCleanup }) => {
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

    const spawned = await api.post<{ pane_id: string; tab_id?: string }>("/agents/spawn", {
      agent: "opencode",
      args: ["--session", sessionId],
      title: `pw-opencode-fallback-${nowMs}`,
    });
    paneCleanup.track(spawned.pane_id);
    const paneId = spawned.pane_id;

    try {
      let boundSessionId: string | null = null;
      for (let i = 0; i < 30; i++) {
        await ui.page.waitForTimeout(1_000);
        const panes = await api.get<{ id: string; agent_session_id?: string }[]>("/panes");
        const pane = panes.find((p) => p.id === paneId);
        if (pane?.agent_session_id) {
          boundSessionId = pane.agent_session_id;
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
      await api.post(`/panes/${paneId}/chat-view`, { enabled: true });
      await api.focusPane(paneId);

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
        `[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`,
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

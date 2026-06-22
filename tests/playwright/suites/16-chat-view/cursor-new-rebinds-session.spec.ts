/**
 * Gate: after cursor /new, the chat view re-binds to the new session.
 *
 * Bug being fixed:
 *   After /new, cursor creates a new session directory on disk. The pane's
 *   agent_session_id is still set to the OLD session id. maybe_detect_agent_session
 *   was gated by `agent_session_id.is_some() → return`, so it NEVER re-detected
 *   the new session → chat stayed bound to the dead old session → new assistant
 *   replies never rendered.
 *
 * Fix:
 *   When agent_session_id.is_some() AND needs=true (hookless disk-polling provider,
 *   e.g. cursor), run a stale-session check: detect_session_id with max_age=30s,
 *   excluding the currently-pinned session and other panes' sessions. After /new,
 *   the new session dir is fresh (<30s), the old one is excluded → exactly 1
 *   candidate → re-bind. Steady-state: no other fresh session → 0 candidates → no
 *   eviction.
 *
 * RED (without fix): agent_session_id stays pinned to session-1 after session-2
 *   dir is created; polling loop times out.
 * GREEN (with fix): stale check fires on next PTY output chunk → finds session-2
 *   → evicts session-1 → binds session-2 → chat shows reply-2.
 *
 * Scenario:
 *   1. Spawn cursor in a unique testCwd (realpath-resolved to avoid macOS symlink
 *      drift between /var and /private/var).
 *   2. Wait for agent_session_id to be set by cursor's hook (cursor calls the
 *      CodeTerm session-start hook with its own session ID on startup).
 *      session-1 = that hook-reported ID.
 *   3. Write a JSONL assistant reply for session-1. Enable chat-view; assert reply-1
 *      appears (proves initial binding + JSONL path resolution both work).
 *   4. Simulate /new: create a fresh uuid2 dir in ~/.cursor/chats/<md5(realCwd)>/.
 *      The stale check sees session-1 as current (excluded), uuid2 as the only
 *      fresh non-excluded candidate → rebinds to uuid2.
 *   5. Type into cursor to produce PTY output, which triggers maybe_detect_agent_session.
 *      Assert agent_session_id changes to uuid2.
 *   6. Write JSONL for uuid2 with "pw-cursor-reply-2". Assert reply-2 visible.
 *
 * macOS web-mode:
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden tests/playwright/suites/16-chat-view/cursor-new-rebinds-session.spec.ts
 */
import crypto from "crypto";
import { existsSync, mkdirSync, realpathSync, rmSync, writeFileSync } from "fs";
import os from "os";
import path from "path";
import { expect, test } from "../../helpers/fixtures";

const CURSOR_AGENT_BIN = "/Users/dev/.local/bin/cursor-agent";
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
  "@golden cursor /new re-binds chat to the new session",
  async ({ api, ui, paneCleanup }) => {
    test.skip(!existsSync(CURSOR_AGENT_BIN), "cursor-agent not installed on this machine");

    // Create a unique temp cwd. Use realpathSync to resolve macOS symlinks
    // (/var → /private/var) so CodeTerm's path matches cursor-agent's `pwd -P` result.
    // Without this, md5(pane.cwd) ≠ md5(cursor's pwd -P), and the chats dir scanned
    // by detectSessionId is a different directory from where cursor writes its sessions.
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

    // Spawn cursor. skip_permissions writes the workspace-trust marker and adds --force
    // to bypass cursor's one-time trust prompt. Pass the realpath cwd so the daemon
    // and cursor-agent agree on the path for chats dir lookups.
    const spawned = await api.post<{ pane_id: string; tab_id?: string }>("/agents/spawn", {
      agent: "cursor",
      cwd: testCwd,
      title: `pw-cursor-new-${nowMs}`,
      skip_permissions: true,
    });
    paneCleanup.track(spawned.pane_id);
    const paneId = spawned.pane_id;

    // Type 'x' then Backspace every second to reliably produce PTY output from the
    // cursor TUI (a printable char + delete guarantees a redraw → maybe_detect_agent_session
    // fires → stale check runs). Plain Esc can be a silent no-op in cursor's idle state.
    const tick = setInterval(async () => {
      await api
        .post(`/panes/${paneId}/send`, { text: "x", enter: false, raw: true })
        .catch(() => {});
      await api
        .post(`/panes/${paneId}/send`, {
          actions: [{ type: "key", key: "backspace" }],
          enter: false,
          raw: true,
        })
        .catch(() => {});
    }, 1_000);

    try {
      // ── Phase 1: wait for agent_session_id to be set ─────────────────────────
      // cursor-agent reports its session via CodeTerm's hook endpoint on startup.
      // The hook fires within ~2s and sets agent_session_id to cursor's real session.
      // We don't pre-create any chats dir — cursor's own hook sets session-1, so the
      // stale check in steady-state finds 0 candidates (chats dir is empty until we
      // create uuid2) → no premature eviction.
      for (let i = 0; i < 30; i++) {
        await ui.page.waitForTimeout(1_000);
        const panes = await api.get<{ id: string; agent_session_id?: string | null }[]>("/panes");
        const pane = panes.find((p) => p.id === paneId);
        if (pane?.agent_session_id) {
          session1 = pane.agent_session_id;
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

      // ── Phase 2: write reply-1 JSONL so chat shows session-1 content ────────
      // The tailer watches jsonlPath(testCwd, session1). If the file exists at that
      // path (or is created after the tailer starts), it reads and renders the content.
      // We write the JSONL before enabling chat-view so the tailer finds it immediately.
      const jp1 = jsonlPath(testCwd, session1!);
      mkdirSync(path.dirname(jp1), { recursive: true });
      writeFileSync(jp1, assistantJsonl(REPLY_1), "utf8");

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

      // Confirms initial binding works: session-1's reply is visible.
      const msg1 = chat
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: REPLY_1 })
        .first();
      await expect(msg1).toBeVisible({ timeout: 30_000 });

      // ── Phase 3: simulate /new ───────────────────────────────────────────────
      // Create session-2's chats dir entry with a fresh meta.json. This is what
      // cursor does on disk when the user runs /new: creates a new session UUID dir.
      //
      // The stale check (fix) in maybe_detect_agent_session:
      //   - current = session1 (set by hook) → stale path
      //   - exclude = [session1, other panes' sessions]
      //   - scan chatsDir: finds uuid2 (fresh, not excluded) → 1 candidate → returns uuid2
      //   - evicts session1, binds uuid2
      //
      // Without the fix: is_some() guard returns early → stays session1 forever.
      const session2Dir = path.join(chatsDir, uuid2);
      mkdirSync(session2Dir, { recursive: true });
      writeFileSync(
        path.join(session2Dir, "meta.json"),
        JSON.stringify({ createdAtMs: Date.now(), updatedAtMs: Date.now() }),
        "utf8",
      );

      // ── Phase 4: wait for stale-session re-detection ─────────────────────────
      // The tick keeps sending 'x'+Backspace → PTY output → maybe_detect_agent_session
      // → stale check runs → finds uuid2 → evicts session1 → binds uuid2.
      let reboundSession: string | null = null;
      for (let i = 0; i < 40; i++) {
        await ui.page.waitForTimeout(1_000);
        const panes = await api.get<{ id: string; agent_session_id?: string | null }[]>("/panes");
        const pane = panes.find((p) => p.id === paneId);
        if (pane?.agent_session_id && pane.agent_session_id !== session1) {
          reboundSession = pane.agent_session_id;
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

      // ── Phase 5: assert reply-2 appears in chat ──────────────────────────────
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

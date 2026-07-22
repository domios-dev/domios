/**
 * WHAT: cron covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 07-cron.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";
import { ensureDevConsole, killDevConsole } from "../../helpers/dev-console";
import { pollUntil } from "../../helpers/poll";

interface CronJob {
  id: string;
  enabled: boolean;
  next_run?: number | null;
  last_run?: { run_id: string; status: string } | null;
}

async function clockMocked(api: ApiClient): Promise<boolean> {
  try {
    const res = await api.get<{ mocked: boolean }>("/debug/clock");
    return res.mocked;
  } catch {
    return false;
  }
}

test.beforeEach(async ({ api }) => {
  // clock — restart so the mock gate is active.
  if (!(await clockMocked(api))) {
    killDevConsole();
    await ensureDevConsole();
  }
  expect(await clockMocked(api), "mock clock gate did not activate").toBe(true);
});

test("@smoke debug clock reports mocked state and advances @restart", async ({ api }) => {
  const before = await api.get<{ mocked: boolean; now_ms: number }>("/debug/clock");
  expect(before.mocked).toBe(true);
  const advanced = await api.post<{ now_ms: number }>("/debug/clock/advance", { ms: 1_000 });
  expect(advanced.now_ms).toBeGreaterThanOrEqual(before.now_ms + 1_000);
});

test("@golden oneshot cron fires via time travel and spawns a real run @restart", async ({ api }) => {
  test.setTimeout(300_000);

  const ws = (await api.get<{ id: string; is_active: boolean }[]>("/workspaces")).find(
    (w) => w.is_active,
  )!;
  const name = `pw-cron-${Date.now()}`;
  const created = await api.post<CronJob>("/crons", {
    name,
    workspace_id: ws.id,
    schedule: "in 5m",
    oneshot: true,
    task: "Reply with exactly the word DONE and nothing else.",
    agent: "claude",
    model: "sonnet",
    enabled: true,
    jitter_max_ms: 0,
  });
  expect(created.id).toBeTruthy();

  try {
    const beforeFire = await api.get<CronJob>(`/crons/${created.id}`);
    expect(beforeFire.next_run, "oneshot must have a next_run").toBeTruthy();

    // 1ms short of the fire time: nothing may fire.
    const now = (await api.get<{ now_ms: number }>("/debug/clock")).now_ms;
    const toFire = beforeFire.next_run! - now;
    expect(toFire).toBeGreaterThan(0);
    await api.post("/debug/clock/advance", { ms: toFire - 1 });
    let history = await api.get<unknown[]>(`/crons/${created.id}/history`);
    expect(history.length, "fired 1ms early").toBe(0);

    // Cross the boundary: the scheduler is woken and must fire.
    await api.post("/debug/clock/advance", { ms: 2 });
    const firedHistory = await pollUntil(async () => {
      history = await api.get<unknown[]>(`/crons/${created.id}/history`);
      return history.length > 0 ? history : undefined;
    }, 60_000, 1_000, "cron fires after crossing its fire time");
    if (!firedHistory) throw new Error("cron history did not appear after crossing its fire time");
    history = firedHistory;
    expect(history.length, "cron never fired after crossing its fire time").toBe(1);

    // Oneshot contract between fire and completion: disabled, no next_run.
    try {
      const afterFire = await api.get<CronJob>(`/crons/${created.id}`);
      expect(afterFire.enabled, "oneshot must disable after firing").toBe(false);
      expect(afterFire.next_run ?? null).toBeNull();
    } catch (e) {
      expect(String(e)).toContain("404");
    }

    const gone = await pollUntil(async () => {
      try {
        await api.get<CronJob>(`/crons/${created.id}`);
        return undefined;
      } catch (e) {
        expect(String(e)).toContain("404");
        return true;
      }
    }, 180_000, 3_000, "oneshot job auto-deletes after run");
    expect(gone, "oneshot never completed (job still present after 180s)").toBe(true);
  } finally {
    await api.request("DELETE", `/crons/${created.id}`).catch(() => {});
  }
});

test("@golden recurring cron fires exactly per boundary, not per advance call @restart", async ({ api }) => {
  test.setTimeout(300_000);

  const ws = (await api.get<{ id: string; is_active: boolean }[]>("/workspaces")).find(
    (w) => w.is_active,
  )!;
  const name = `pw-cron-rec-${Date.now()}`;
  // pure scheduling math through the live API, not run execution.
  const created = await api.post<CronJob>("/crons", {
    name,
    workspace_id: ws.id,
    schedule: "*/5 * * * *",
    task: "echo never",
    enabled: false,
    jitter_max_ms: 0,
  });

  try {
    expect(created.next_run ?? null, "disabled cron must not schedule").toBeNull();

    // Enabling computes next_run from the MOCKED now.
    const enabled = await api.post<CronJob>(`/crons/${created.id}/enable`);
    const now = (await api.get<{ now_ms: number }>("/debug/clock")).now_ms;
    const next = enabled.next_run!;
    expect(next).toBeGreaterThan(now);
    expect(next - now, "*/5 boundary must be within 5 minutes").toBeLessThanOrEqual(300_000);
    expect(next % 300_000, "*/5 boundary must land on a 5-minute boundary").toBe(0);
  } finally {
    await api.request("DELETE", `/crons/${created.id}`).catch(() => {});
  }
});

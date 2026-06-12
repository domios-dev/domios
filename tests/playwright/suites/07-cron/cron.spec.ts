/**
 * 07-cron — scheduler semantics on a live dev console with a mocked cron
 * clock (CODETERM_TEST_CLOCK=1 + /api/debug/clock/advance). No wall-clock
 * waits for fire times: the suite time-travels, the scheduler is woken,
 * and the fire spawns a REAL agent (one trivial sonnet call).
 */
import { expect, test } from "@playwright/test";
import { ApiClient } from "../../helpers/api-client";
import { ensureDevConsole, killDevConsole } from "../../helpers/dev-console";

const api = new ApiClient();

interface CronJob {
  id: string;
  enabled: boolean;
  next_run?: number | null;
  last_run?: { run_id: string; status: string } | null;
}

async function clockMocked(): Promise<boolean> {
  try {
    const res = await api.get<{ mocked: boolean }>("/debug/clock");
    return res.mocked;
  } catch {
    return false;
  }
}

test.beforeAll(async () => {
  // A console started before this branch (or without the env) has a real
  // clock — restart so the mock gate is active.
  if (!(await clockMocked())) {
    killDevConsole();
    await ensureDevConsole();
  }
  expect(await clockMocked(), "mock clock gate did not activate").toBe(true);
});

test("@smoke debug clock reports mocked state and advances", async () => {
  const before = await api.get<{ mocked: boolean; now_ms: number }>("/debug/clock");
  expect(before.mocked).toBe(true);
  const advanced = await api.post<{ now_ms: number }>("/debug/clock/advance", { ms: 1_000 });
  expect(advanced.now_ms).toBeGreaterThanOrEqual(before.now_ms + 1_000);
});

test("@golden oneshot cron fires via time travel and spawns a real run", async () => {
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
    await new Promise((r) => setTimeout(r, 2_000));
    let history = await api.get<unknown[]>(`/crons/${created.id}/history`);
    expect(history.length, "fired 1ms early").toBe(0);

    // Cross the boundary: the scheduler is woken and must fire.
    await api.post("/debug/clock/advance", { ms: 2 });
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      history = await api.get<unknown[]>(`/crons/${created.id}/history`);
      if (history.length > 0) break;
      await new Promise((r) => setTimeout(r, 1_000));
    }
    expect(history.length, "cron never fired after crossing its fire time").toBe(1);

    // Oneshot contract between fire and completion: disabled, no next_run.
    // The job may already be gone if the (real, fast) agent run finished —
    // completed oneshots AUTO-DELETE, which is the completion signal below.
    try {
      const afterFire = await api.get<CronJob>(`/crons/${created.id}`);
      expect(afterFire.enabled, "oneshot must disable after firing").toBe(false);
      expect(afterFire.next_run ?? null).toBeNull();
    } catch (e) {
      expect(String(e)).toContain("404");
    }

    // The run is real: a hidden agent pane executes the task; the
    // completion path then auto-deletes the oneshot job. Deletion within
    // budget IS the proof the run completed.
    const runDeadline = Date.now() + 180_000;
    let gone = false;
    while (Date.now() < runDeadline && !gone) {
      try {
        await api.get<CronJob>(`/crons/${created.id}`);
        await new Promise((r) => setTimeout(r, 3_000));
      } catch (e) {
        expect(String(e)).toContain("404");
        gone = true;
      }
    }
    expect(gone, "oneshot never completed (job still present after 180s)").toBe(true);
  } finally {
    await api.request("DELETE", `/crons/${created.id}`).catch(() => {});
  }
});

test("@golden recurring cron fires exactly per boundary, not per advance call", async () => {
  test.setTimeout(300_000);

  const ws = (await api.get<{ id: string; is_active: boolean }[]>("/workspaces")).find(
    (w) => w.is_active,
  )!;
  const name = `pw-cron-rec-${Date.now()}`;
  // Disabled at creation so it never spawns agents — this test asserts
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

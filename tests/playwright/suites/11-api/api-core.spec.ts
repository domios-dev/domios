/**
 * 11-api core — REST contract coverage against the live dev console.
 * No UI, no mock provider. Per testplan §3.13: happy path, auth,
 * validation, not-found, idempotency per cluster.
 *
 * Lane fence: /api/plugin*, /api/mesh/*, /api/update/* are NOT covered
 * here (plugin contract is in flight on Mac-Air; mesh needs the §4.2 peer
 * simulator; update mutates the install).
 */
import { expect, test } from "@playwright/test";
import { ApiClient, readDevToken } from "../../helpers/api-client";
import { API_BASE } from "../../helpers/dev-console";
import { apiReset } from "../../helpers/reset";

const api = new ApiClient();

async function raw(
  method: string,
  path: string,
  opts: { token?: string | null; body?: unknown } = {},
): Promise<{ status: number; body: string }> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (opts.token !== null) {
    headers.Authorization = `Bearer ${opts.token ?? readDevToken()}`;
  }
  const r = await fetch(`${API_BASE}${path}`, {
    method,
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
    signal: AbortSignal.timeout(15_000),
  });
  return { status: r.status, body: await r.text() };
}

test.afterAll(async () => {
  await apiReset(api);
});

// ── auth contract ────────────────────────────────────────────────────

const PROTECTED_GETS = ["/api/panes", "/api/workspaces", "/api/activity", "/api/crons", "/api/themes"];

test("@smoke protected routes reject a missing bearer with 401", async () => {
  for (const path of PROTECTED_GETS) {
    const { status } = await raw("GET", path, { token: null });
    expect(status, `${path} without token`).toBe(401);
  }
});

test("@smoke protected routes reject a garbage bearer with 401", async () => {
  for (const path of PROTECTED_GETS) {
    const { status } = await raw("GET", path, { token: "not-a-real-token" });
    expect(status, `${path} with garbage token`).toBe(401);
  }
});

test("@smoke public liveness routes answer without auth", async () => {
  const health = await raw("GET", "/api/health", { token: null });
  expect(health.status).toBe(200);
  expect(JSON.parse(health.body).status).toBe("ok");
});

// ── panes: not-found + PTY echo round-trip ───────────────────────────

test("@golden pane ops on a nonexistent id return 404", async () => {
  for (const op of ["focus", "close"]) {
    const { status } = await raw("POST", `/api/panes/no-such-pane/${op}`);
    expect(status, `POST ${op}`).toBe(404);
  }
  const { status } = await raw("GET", "/api/panes/no-such-pane/output");
  expect(status, "GET output").toBe(404);
});

test("@golden PTY echo round-trip: send a command, read it back from output", async () => {
  test.setTimeout(120_000);
  const created = (await api.createPane()) as { id: string };
  const marker = `pw-api-${Date.now()}`;
  try {
    // Give the shell a moment to be ready, then echo a unique marker.
    await new Promise((r) => setTimeout(r, 3_000));
    await api.post(`/panes/${created.id}/send`, { text: `echo ${marker}`, enter: true });

    let seen = false;
    for (let i = 0; i < 30 && !seen; i++) {
      await new Promise((r) => setTimeout(r, 1_000));
      const out = await api.get<{ text?: string } | string>(`/panes/${created.id}/output`);
      const text = typeof out === "string" ? out : (out.text ?? JSON.stringify(out));
      seen = text.includes(marker);
    }
    expect(seen, `marker ${marker} never appeared in pane output`).toBe(true);
  } finally {
    await api.closePane(created.id).catch(() => {});
  }
});

// ── workspaces: validation + not-found + idempotency ─────────────────

test("@golden workspace ops on a nonexistent id return 404, close is not idempotent-silent", async () => {
  expect((await raw("POST", "/api/workspaces/no-such-ws/close")).status).toBe(404);
  expect(
    (await raw("POST", "/api/workspaces/no-such-ws/rename", { body: { name: "x" } })).status,
  ).toBe(404);

  const name = `pw-api-ws-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  expect(ws).toBeDefined();
  expect((await raw("POST", `/api/workspaces/${ws!.id}/close`)).status).toBe(200);
  // Second close: the workspace is gone — must be 404, not a silent 200.
  expect((await raw("POST", `/api/workspaces/${ws!.id}/close`)).status).toBe(404);
});

// ── crons: CRUD lifecycle (disabled — nothing may fire mid-test) ─────

test("@golden cron CRUD lifecycle", async () => {
  const ws = (await api.workspaces()).find((w) => w.is_active)!;
  const name = `pw-api-cron-${Date.now()}`;
  const created = await api.post<{ id: string; enabled: boolean }>("/crons", {
    name,
    workspace_id: ws.id,
    schedule: "0 0 1 1 *",
    task: "echo never",
    enabled: false,
  });
  expect(created.id).toBeTruthy();
  expect(created.enabled).toBe(false);

  const listed = await api.get<{ id: string }[]>("/crons");
  expect(listed.some((c) => c.id === created.id)).toBe(true);

  expect((await raw("POST", `/api/crons/${created.id}/enable`)).status).toBe(200);
  expect((await raw("POST", `/api/crons/${created.id}/disable`)).status).toBe(200);

  expect((await raw("DELETE", `/api/crons/${created.id}`)).status).toBe(200);
  expect((await raw("GET", `/api/crons/${created.id}`)).status).toBe(404);
});

test("@golden cron create with a malformed schedule is rejected with 400", async () => {
  const ws = (await api.workspaces()).find((w) => w.is_active)!;
  const { status } = await raw("POST", "/api/crons", {
    body: {
      name: "pw-bad-cron",
      workspace_id: ws.id,
      schedule: "not a cron expr",
      task: "echo x",
      enabled: false,
    },
  });
  expect(status).toBe(400);
});

// ── pasteboard: put/get/pop/delete cycle ─────────────────────────────

test("@golden pasteboard put → get → pop → gone", async () => {
  const body = `pw-paste-${Date.now()}`;
  const created = await api.post<{ id: string }>("/pasteboard", { body });
  expect(created.id).toBeTruthy();

  const fetched = await api.get<{ body: string }>(`/pasteboard/${created.id}`);
  expect(fetched.body).toBe(body);

  // pop=true returns the content and deletes the entry.
  const popped = await api.get<{ body: string }>(`/pasteboard/${created.id}?pop=true`);
  expect(popped.body).toBe(body);
  expect((await raw("GET", `/api/pasteboard/${created.id}`)).status).toBe(404);

  // Deleting an already-gone paste is an honest 404.
  expect((await raw("DELETE", `/api/pasteboard/${created.id}`)).status).toBe(404);
});

// ── read-only feeds: activity, themes, sessions search ──────────────

test("@golden activity feed returns a bounded array and accepts source filter", async () => {
  const all = await api.get<unknown[]>("/activity");
  expect(Array.isArray(all)).toBe(true);
  expect(all.length).toBeLessThanOrEqual(10_000);
  const filtered = await api.get<unknown[]>("/activity?source=cron");
  expect(Array.isArray(filtered)).toBe(true);
});

test("@golden themes list is a non-empty name array", async () => {
  const res = await api.get<{ themes: string[] }>("/themes");
  expect(Array.isArray(res.themes)).toBe(true);
  expect(res.themes.length).toBeGreaterThan(0);
  expect(res.themes).toContain("github-dark");
});

// ── uploads (browser attachments / voice notes) ──────────────────────

test("@golden upload round-trip: multipart in, exact bytes back via serve-file", async () => {
  const content = `pw-upload-${Date.now()}`;
  const form = new FormData();
  form.append("file", new Blob([content], { type: "text/plain" }), "pw upload (тест).txt");
  const res = await fetch(`${API_BASE}/api/upload`, {
    method: "POST",
    headers: { Authorization: `Bearer ${readDevToken()}` },
    body: form,
    signal: AbortSignal.timeout(15_000),
  });
  expect(res.status).toBe(200);
  const { path, size } = (await res.json()) as { path: string; size: number };
  expect(size).toBe(content.length);
  expect(path).toContain("uploads");
  // Sanitizer: nothing shell-hostile survives in the stored name.
  const stored = path.split(/[\\/]/).pop()!;
  expect(stored).toMatch(/^[A-Za-z0-9._-]+$/);

  // Uploads live inside the data dir, so serve-file can hand them back.
  const back = await fetch(
    `${API_BASE}/api/serve-file?path=${encodeURIComponent(path)}`,
    { headers: { Authorization: `Bearer ${readDevToken()}` } },
  );
  expect(back.status).toBe(200);
  expect(await back.text()).toBe(content);
});

test("@golden upload rejects a missing file field and missing auth", async () => {
  const form = new FormData();
  form.append("not-file", new Blob(["x"]), "x.txt");
  const res = await fetch(`${API_BASE}/api/upload`, {
    method: "POST",
    headers: { Authorization: `Bearer ${readDevToken()}` },
    body: form,
    signal: AbortSignal.timeout(15_000),
  });
  expect(res.status).toBe(400);

  const unauth = await fetch(`${API_BASE}/api/upload`, {
    method: "POST",
    body: new FormData(),
    signal: AbortSignal.timeout(15_000),
  });
  expect(unauth.status).toBe(401);
});

// ── fs browse (web folder picker) ────────────────────────────────────

test("@golden fs list-dir + mkdir: browse, create, conflict, validation", async () => {
  const home = await raw("GET", "/api/fs/list-dir");
  expect(home.status).toBe(200);
  const listing = JSON.parse(home.body) as { path: string; entries: unknown[]; roots: unknown[] };
  expect(listing.path.length).toBeGreaterThan(0);
  expect(Array.isArray(listing.entries)).toBe(true);
  expect(listing.roots.length).toBeGreaterThan(0);

  // Create under the OS temp dir — never touch real user folders.
  const tmpBase = JSON.parse(
    (await raw("GET", `/api/fs/list-dir?path=${encodeURIComponent(process.env.TEMP ?? "/tmp")}`)).body,
  ) as { path: string };
  const name = `pw-mkdir-${Date.now()}`;
  const created = await raw("POST", "/api/fs/mkdir", { body: { parent: tmpBase.path, name } });
  expect(created.status).toBe(200);
  const createdPath = (JSON.parse(created.body) as { path: string }).path;
  expect(createdPath).toContain(name);

  // The new folder is listable and empty.
  const inside = await raw("GET", `/api/fs/list-dir?path=${encodeURIComponent(createdPath)}`);
  expect(inside.status).toBe(200);
  expect((JSON.parse(inside.body) as { entries: unknown[] }).entries).toHaveLength(0);

  // Conflict and validation are honest errors, not silent successes.
  const dup = await raw("POST", "/api/fs/mkdir", { body: { parent: tmpBase.path, name } });
  expect(dup.status).toBe(409);
  for (const bad of ["", "..", "a/b", "x:y"]) {
    const r = await raw("POST", "/api/fs/mkdir", { body: { parent: tmpBase.path, name: bad } });
    expect(r.status, `name ${JSON.stringify(bad)}`).toBe(400);
  }
  const unauth = await raw("POST", "/api/fs/mkdir", {
    token: null,
    body: { parent: tmpBase.path, name: "nope" },
  });
  expect(unauth.status).toBe(401);
});

test("@golden sessions search answers with a hits array for any query", async () => {
  const res = await api.get<{ hits: unknown[] }>(
    "/sessions/search?q=pw-definitely-unmatched-term",
  );
  expect(Array.isArray(res.hits)).toBe(true);
  expect(res.hits.length).toBe(0);
});

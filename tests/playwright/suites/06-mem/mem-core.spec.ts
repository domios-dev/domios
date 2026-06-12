/**
 * 06-mem core — save/get/update/delete + search + graph relate against the
 * dev console's OWN mem store (~/.codeterm-dev — fully isolated from prod
 * mem). Every item this suite creates is deleted in afterAll.
 *
 * Search assertions are rank-tolerant by design (§3.6): vector scores are
 * float-fuzzy and the embedding worker is async, so tests assert "top-K
 * contains id", never exact rank.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";

const created: string[] = [];

interface MemItem {
  id: string;
  title: string;
  body: string;
  kind: string;
}
interface SearchHit {
  item: MemItem;
}

async function memSave(api: { post<T>(p: string, body?: unknown): Promise<T> }, kind: string, title: string, body: string): Promise<string> {
  const res = await api.post<{ id: string }>("/mem/items", { kind, title, body });
  expect(res.id).toBeTruthy();
  created.push(res.id);
  return res.id;
}

test.afterEach(async ({ api }) => {
  for (const id of created) {
    await api.request("DELETE", `/mem/items/${id}`).catch(() => {});
  }
  created.length = 0;
});

test("@golden save → get round-trip preserves kind/title/body", async ({ api }) => {
  const id = await memSave(api, "fact", "pw mem roundtrip probe", "the quick zebra audits the graph");
  const item = await api.get<MemItem>(`/mem/items/${id}`);
  expect(item.id).toBe(id);
  expect(item.kind).toBe("fact");
  expect(item.title).toBe("pw mem roundtrip probe");
  expect(item.body).toContain("zebra audits");
});

test("@golden update mutates the body and history records it", async ({ api }) => {
  const id = await memSave(api, "fact", "pw mem update probe", "version one of the body");
  await api.request("PATCH", `/mem/items/${id}`, { body: "version two of the body" });
  const item = await api.get<MemItem>(`/mem/items/${id}`);
  expect(item.body).toBe("version two of the body");

  const history = await api.get<unknown>(`/mem/items/${id}/history`);
  const entries = Array.isArray(history)
    ? history
    : ((history as { history?: unknown[] }).history ?? []);
  expect(entries.length, "update must leave a history trail").toBeGreaterThan(0);
});

test("@golden search finds a freshly saved fact by distinctive terms (rank-tolerant)", async ({ api }) => {
  const marker = `quokka-canary-${Date.now()}`;
  const id = await memSave(
    api,
    "fact",
    `pw search probe ${marker}`,
    `Distinctive testing fact: the ${marker} harness exercises hybrid FTS and vector recall.`,
  );

  const found = await pollUntil(async () => {
    const res = await api.get<{ hits: SearchHit[] }>(
      `/mem/search?q=${encodeURIComponent(marker + " harness")}`,
    );
    return res.hits.slice(0, 5).some((h) => h.item.id === id);
  }, 10_000, 1_000, `mem search top-5 contains ${id}`);
  expect(found, `id ${id} not in top-5 for its own distinctive terms`).toBe(true);
});

test("@golden delete removes the item from get and search", async ({ api }) => {
  const marker = `wombat-delete-${Date.now()}`;
  const id = await memSave(api, "fact", `pw delete probe ${marker}`, `${marker} body to be removed`);

  const del = await api.request<{ deleted: boolean }>("DELETE", `/mem/items/${id}`);
  expect(del.deleted).toBe(true);

  // get must 404 now
  let status = 200;
  try {
    await api.get(`/mem/items/${id}`);
  } catch (e) {
    status = /HTTP (\d+)/.exec(String(e)) ? Number(/HTTP (\d+)/.exec(String(e))![1]) : 0;
  }
  expect(status).toBe(404);

  const res = await api.get<{ hits: SearchHit[] }>(`/mem/search?q=${encodeURIComponent(marker)}`);
  expect(res.hits.some((h) => h.item.id === id)).toBe(false);
});

test("@golden wake-up and stats answer with content", async ({ api }) => {
  const wake = await api.get<Record<string, unknown>>("/mem/wake-up");
  expect(typeof wake).toBe("object");
  const stats = await api.get<Record<string, unknown>>("/mem/stats");
  expect(typeof stats).toBe("object");
  expect(Object.keys(stats).length).toBeGreaterThan(0);
});

test("@golden graph relate links two items and related returns the edge", async ({ api }) => {
  const a = await memSave(api, "fact", "pw graph node alpha", "alpha node for relation test");
  const b = await memSave(api, "fact", "pw graph node beta", "beta node for relation test");

  await api.post("/mem/graph/relate", { from: a, rel: "relates_to", to: b });

  const related = await api.get<unknown>(`/mem/graph/related?id=${encodeURIComponent(a)}`);
  const text = JSON.stringify(related);
  expect(text, `edge ${a} -> ${b} not visible in related()`).toContain(b);
});

// Multi-tab live UX verifier for the Chat streaming preview (ux-verifier role).
//
// Watches EVERY tab that emits chat-stream-preview events on the dev daemon,
// reproduces the exact UI pipeline (displayablePreview + uncoveredPreviewTail +
// slice(-8) + both useStreamPreview death rules), and scores per tab per turn:
// jank frames (honest classifier: full-window replacement counts), dupe rows,
// leftover rows at swap-death, height thrash (row-count changes), lead per
// turn. Also appends every raw preview event to a corpus ndjson for offline
// replay by matcher-analyst.
//
// Run: npx tsx ux-live-verify2.ts --token <TOKEN> --outdir <dir>

import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import {
  canonicalBlob,
  capPreviewRows,
  createPreviewStabilizerState,
  displayablePreview,
  isSmoothPreviewTransition,
  stabilizePreview,
  uncoveredPreviewTail,
} from "/Users/user/Developer/codeterm-worktrees/lane-streaming-consilium/src/components/Terminal/streamPreviewMatch.ts";

const MAX_RENDER_ROWS = 8;
const BASE = "http://127.0.0.1:7686";

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
}
const TOKEN = arg("token");
const OUTDIR = arg("outdir") ?? ".";
if (!TOKEN) {
  console.error("usage: tsx ux-live-verify2.ts --token <TOKEN> [--outdir dir]");
  process.exit(2);
}
mkdirSync(OUTDIR, { recursive: true });
const HEADERS = { Authorization: `Bearer ${TOKEN}` };

/** Honest smoothness: cut-from-top / append-at-bottom / extend-last-row with at
 * least one surviving anchor row; full-window replacement is jank. */
function isSmoothTransition(prev: string[], next: string[]): boolean {
  if (prev.length === 0 || next.length === 0) return true;
  for (let drop = 0; drop < prev.length; drop++) {
    const kept = prev.slice(drop);
    if (kept.length > next.length) continue;
    let matches = true;
    for (let i = 0; i < kept.length; i++) {
      const isLastKept = i === kept.length - 1;
      const same = next[i] === kept[i];
      const extended = isLastKept && next[i].startsWith(kept[i]);
      if (!same && !extended) {
        matches = false;
        break;
      }
    }
    if (matches) return true;
  }
  return false;
}

interface PreviewEvent {
  tab_id: string;
  turn_seq: number;
  frozen: string[];
  live: string[];
}

interface TurnStat {
  seq: number;
  firstPreview: number | null;
  firstCanonAfterPreview: number | null;
  peak: number;
  jank: number;
  heightThrash: number;
  dupes: number;
  leftoverAtDeath: number | null;
}

interface TabState {
  stab: ReturnType<typeof createPreviewStabilizerState>;
  event: PreviewEvent | null;
  blob: string;
  displayed: string[];
  working: boolean;
  status: string;
  agentType: string;
  title: string;
  turns: Map<number, TurnStat>;
  eventCount: number;
}

const tabs = new Map<string, TabState>();
const eventLogPath = join(OUTDIR, "ux-events.ndjson");
const corpusPath = join(OUTDIR, "preview-corpus.ndjson");

function logEvent(tabId: string, kind: string, extra: Record<string, unknown> = {}): void {
  appendFileSync(
    eventLogPath,
    JSON.stringify({ t: Date.now(), tab: tabId, kind, ...extra }) + "\n",
  );
}

function stateFor(tabId: string): TabState {
  let s = tabs.get(tabId);
  if (!s) {
    s = {
      stab: createPreviewStabilizerState(),
      event: null,
      blob: "",
      displayed: [],
      working: false,
      status: "?",
      agentType: "?",
      title: "?",
      turns: new Map(),
      eventCount: 0,
    };
    tabs.set(tabId, s);
    logEvent(tabId, "tracking-started");
  }
  return s;
}

function turnFor(s: TabState, seq: number): TurnStat {
  let t = s.turns.get(seq);
  if (!t) {
    t = {
      seq,
      firstPreview: null,
      firstCanonAfterPreview: null,
      peak: 0,
      jank: 0,
      heightThrash: 0,
      dupes: 0,
      leftoverAtDeath: null,
    };
    s.turns.set(seq, t);
  }
  return t;
}

function recompute(tabId: string, reason: string): void {
  const s = stateFor(tabId);
  const content = s.event ? displayablePreview(s.event.frozen.concat(s.event.live)) : [];
  const next = s.event
    ? stabilizePreview(
        s.stab,
        s.event.turn_seq,
        capPreviewRows(uncoveredPreviewTail(content, s.blob)),
        content.length > 0,
        s.blob,
      )
    : [];
  const changed = s.displayed.join("\n") !== next.join("\n");
  if (changed && s.event) {
    const t = turnFor(s, s.event.turn_seq);
    if (s.displayed.length === 0 && next.length > 0) {
      logEvent(tabId, "birth", { seq: s.event.turn_seq });
      if (t.firstPreview === null) t.firstPreview = Date.now();
    }
    if (s.displayed.length !== next.length) t.heightThrash++;
    if (!isSmoothPreviewTransition(s.displayed, next)) {
      t.jank++;
      logEvent(tabId, "jank", {
        seq: s.event.turn_seq,
        reason,
        prev: s.displayed.slice(0, 8),
        next: next.slice(0, 8),
      });
    }
    t.peak = Math.max(t.peak, next.length);
    for (const row of next) {
      const n = row.replace(/\s+/g, " ").trim();
      if (n.length >= 16 && s.blob.includes(n)) {
        t.dupes++;
        logEvent(tabId, "dupe", { seq: s.event.turn_seq, row: n.slice(0, 140) });
      }
    }
  }
  s.displayed = next;
  if (!s.working && s.event !== null && s.displayed.length === 0) {
    logEvent(tabId, "death-covered", { seq: s.event.turn_seq });
    s.event = null;
  }
}

function onBlobChange(tabId: string, prevBlob: string): void {
  const s = stateFor(tabId);
  if (s.blob !== prevBlob && !s.working && s.event !== null) {
    const t = turnFor(s, s.event.turn_seq);
    t.leftoverAtDeath = s.displayed.length;
    logEvent(tabId, "death-swap", {
      seq: s.event.turn_seq,
      leftoverRows: s.displayed.length,
      leftover: s.displayed.slice(0, 6),
    });
    s.event = null;
    s.displayed = [];
  }
}

async function pollLoop(): Promise<void> {
  for (;;) {
    try {
      const tabsRes = await fetch(`${BASE}/api/tabs`, { headers: HEADERS });
      if (tabsRes.ok) {
        const list = (await tabsRes.json()) as {
          id: string;
          status?: string;
          agent_type?: string;
          title?: string;
        }[];
        for (const [tabId, s] of tabs) {
          const rec = list.find((t) => t.id === tabId);
          if (!rec) continue;
          s.agentType = rec.agent_type ?? "?";
          s.title = rec.title ?? "?";
          const status = rec.status ?? "?";
          if (status !== s.status) {
            logEvent(tabId, "status-change", { from: s.status, to: status });
            s.status = status;
          }
          const working = rec.status === "working";
          if (working !== s.working) {
            s.working = working;
            logEvent(tabId, "agent-state", { working });
            recompute(tabId, "agent-state");
          }
        }
      }
      for (const [tabId, s] of tabs) {
        const chatRes = await fetch(`${BASE}/api/tabs/${tabId}/chat`, { headers: HEADERS });
        if (!chatRes.ok) continue;
        const chat = (await chatRes.json()) as { messages?: { content?: string }[] };
        const contents = (chat.messages ?? []).slice(-20).map((m) => m.content ?? "");
        const prevBlob = s.blob;
        const nextBlob = canonicalBlob(contents);
        if (nextBlob !== s.blob) {
          s.blob = nextBlob;
          logEvent(tabId, "canon-update", { messages: contents.length });
          if (s.event) {
            const t = turnFor(s, s.event.turn_seq);
            if (t.firstPreview !== null && t.firstCanonAfterPreview === null)
              t.firstCanonAfterPreview = Date.now();
          }
          recompute(tabId, "canon");
          onBlobChange(tabId, prevBlob);
        }
      }
    } catch (e) {
      logEvent("-", "poll-error", { error: String(e) });
    }
    await new Promise((r) => setTimeout(r, 300));
  }
}

const seenNotifications = new Set<string>();
let notifBaseline = false;
async function notifLoop(): Promise<void> {
  for (;;) {
    try {
      const res = await fetch(`${BASE}/api/notifications`, { headers: HEADERS });
      if (res.ok) {
        const list = (await res.json()) as {
          id: string;
          tab_id?: string;
          title?: string;
          body?: string;
          level?: string;
          timestamp?: number;
        }[];
        for (const n of list) {
          if (seenNotifications.has(n.id)) continue;
          seenNotifications.add(n.id);
          if (notifBaseline)
            logEvent(n.tab_id ?? "-", "notification", {
              id: n.id,
              title: n.title,
              body: (n.body ?? "").slice(0, 200),
              level: n.level,
              ts: n.timestamp,
            });
        }
        notifBaseline = true;
      }
    } catch {
      /* daemon down — poll again */
    }
    await new Promise((r) => setTimeout(r, 500));
  }
}

async function sseLoop(): Promise<void> {
  for (;;) {
    try {
      const res = await fetch(`${BASE}/api/events/stream`, { headers: HEADERS });
      if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
      logEvent("-", "sse-connected");
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let idx;
        while ((idx = buf.indexOf("\n\n")) >= 0) {
          const frame = buf.slice(0, idx);
          buf = buf.slice(idx + 2);
          const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
          if (!dataLine) continue;
          let msg: { event_type?: string; data?: unknown };
          try {
            msg = JSON.parse(dataLine.slice(5).trim());
          } catch {
            continue;
          }
          if (msg.event_type !== "chat-stream-preview") continue;
          const payload = msg.data as PreviewEvent;
          appendFileSync(corpusPath, JSON.stringify({ t: Date.now(), ...payload }) + "\n");
          const s = stateFor(payload.tab_id);
          s.event = payload;
          s.eventCount++;
          recompute(payload.tab_id, "preview-event");
        }
      }
    } catch (e) {
      logEvent("-", "sse-error", { error: String(e) });
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
}

function report(): void {
  const out: Record<string, unknown> = {};
  for (const [tabId, s] of tabs) {
    const turns = [...s.turns.values()].map((t) => ({
      seq: t.seq,
      lead_ms:
        t.firstPreview !== null && t.firstCanonAfterPreview !== null
          ? t.firstCanonAfterPreview - t.firstPreview
          : null,
      peak_rows: t.peak,
      jank: t.jank,
      height_thrash: t.heightThrash,
      dupes: t.dupes,
      leftover_at_death: t.leftoverAtDeath,
    }));
    const leads = turns.map((t) => t.lead_ms).filter((v): v is number => v !== null);
    out[tabId] = {
      agent: s.agentType,
      title: s.title,
      events: s.eventCount,
      turns,
      totals: {
        jank: turns.reduce((a, t) => a + t.jank, 0),
        dupes: turns.reduce((a, t) => a + t.dupes, 0),
        height_thrash: turns.reduce((a, t) => a + t.height_thrash, 0),
        leftover_turns: turns.filter((t) => (t.leftover_at_death ?? 0) > 0).length,
        lead_min: leads.length ? Math.min(...leads) : null,
        lead_max: leads.length ? Math.max(...leads) : null,
        max_rows: Math.max(0, ...turns.map((t) => t.peak_rows)),
      },
    };
  }
  writeFileSync(join(OUTDIR, "ux-live-summary.json"), JSON.stringify(out, null, 2));
}

setInterval(report, 5000);
process.on("SIGINT", () => {
  report();
  process.exit(0);
});
process.on("SIGTERM", () => {
  report();
  process.exit(0);
});

void pollLoop();
void sseLoop();
void notifLoop();
console.log(`ux-live-verify2: watching ALL preview-emitting tabs; outdir ${OUTDIR}`);

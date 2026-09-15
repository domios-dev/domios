// Replays a Claude session whose queued-command echoes are written after the output that followed their sends.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { ChatSend } from "./sendOrder";
import {
  createTranscriptState,
  projectTranscript,
  reduceTranscript,
  transcriptRows,
  type TranscriptState,
} from "./transcript";
import type { NormalizedChatMessage } from "./types";

let passed = 0;
let failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed += 1;
    console.log(`  ✓ ${name}`);
  } else {
    failed += 1;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

interface RichBlock { kind: string; data: Record<string, unknown> }
interface RichRow {
  role: string;
  blocks: RichBlock[];
  uuid: string | null;
  timestamp: string | null;
  timelineMarker?: NormalizedChatMessage["timelineMarker"] | null;
}
interface SessionParser { parseSessionDelta(chunk: string): { messages: RichRow[] } }

const SESSION = { sourceSessionId: "fixture-session", sessionBindingNonce: "nonce" };
const FIXTURE = fileURLToPath(new URL("../../../plugins/provider-claude/tests/fixtures/queued-mid-turn.jsonl", import.meta.url));
const at = (iso: string) => Date.parse(`2026-09-11T18:19:${iso}Z`);
const SENDS: ChatSend[] = [
  { id: "opt-linter", content: "also run the linter", sentAtMs: at("32.850"), correlationId: "opt-linter" },
  { id: "opt-changelog", content: "then update the changelog", sentAtMs: at("38.900"), correlationId: "opt-changelog" },
  { id: "opt-tag", content: "and tag the release", sentAtMs: at("39.950"), correlationId: "opt-tag" },
];
/** The send accepted after each fixture line index. */
const SENT_AFTER_LINE = new Map<number, ChatSend[]>([[2, [SENDS[0]]], [7, [SENDS[1], SENDS[2]]]]);
const EXPECTED = [
  "build the release",
  "Starting the release build.",
  "call:toolu_fixture_build",
  "also run the linter",
  "result:toolu_fixture_build",
  "Running the linter as requested.",
  "call:toolu_fixture_lint",
  "then update the changelog",
  "and tag the release",
  "result:toolu_fixture_lint",
  "Changelog updated and release tagged.",
];

// Mirrors the host's rich-row flattening for the fields ordering reads; tab_chat tests pin the Rust side.
function flatten(rows: RichRow[]): NormalizedChatMessage[] {
  return rows.flatMap((row) => row.blocks.flatMap((block, index): NormalizedChatMessage[] => {
    const base = {
      id: `${row.uuid}-${index}`,
      timestamp: row.timestamp ?? undefined,
      timelineMarker: row.timelineMarker ?? undefined,
    };
    if (block.kind === "text") {
      const type = row.role === "user" ? "user" : row.role === "status" ? "status" : "assistant";
      return [{ ...base, type, content: String(block.data.text ?? "") }];
    }
    if (block.kind === "tool_use") {
      return [{ ...base, type: "tool_call", content: String(block.data.name ?? ""), toolId: block.data.id as string }];
    }
    if (block.kind === "tool_result") {
      return [{ ...base, type: "tool_result", content: "", toolId: block.data.tool_use_id as string }];
    }
    return [];
  }));
}

function label(row: NormalizedChatMessage): string {
  if (row.type === "tool_call") return `call:${row.toolId}`;
  if (row.type === "tool_result") return `result:${row.toolId}`;
  return row.content;
}

function isSubsequence(part: string[], whole: string[]): boolean {
  let cursor = 0;
  for (const item of whole) if (item === part[cursor]) cursor += 1;
  return cursor === part.length;
}

function recordEchoes(sends: ChatSend[], state: TranscriptState): ChatSend[] {
  const { echoes } = projectTranscript(state, sends);
  return sends.map((send) => (send.echo || !echoes.has(send.id) ? send : { ...send, echo: echoes.get(send.id) }));
}

function projectedRows(state: TranscriptState, sends: readonly ChatSend[]) {
  return projectTranscript(state, sends).rows.map((row) => ({ id: row.id, type: row.type, content: row.content, timestamp: row.timestamp }));
}

async function main(): Promise<void> {
  (globalThis as { host?: unknown }).host ??= { manifestJson: () => "null", log: () => undefined };
  const plugin = await import("../../../plugins/provider-claude/src/plugin");
  const parser = plugin.default as unknown as SessionParser;
  const raw = readFileSync(FIXTURE, "utf8");
  const lines = raw.split("\n").filter(Boolean);
  let offset = 0;
  const entries = lines.map((line) => {
    const start = offset;
    offset += line.length + 1;
    return { rows: flatten(parser.parseSessionDelta(line).messages), start, end: offset };
  });

  // ── Live: each line lands as its own delta while the sends are accepted mid-turn.
  {
    let state = createTranscriptState();
    let sends: ChatSend[] = [];
    let cursor = 0;
    const steps: string[][] = [];
    entries.forEach((entry, index) => {
      if (entry.rows.length > 0) {
        const before = projectTranscript(state, sends).rows;
        state = reduceTranscript(state, {
          kind: "delta",
          authority: SESSION,
          range: { start: cursor, end: entry.end },
          rows: entry.rows,
        }).state;
        cursor = entry.end;
        const after = projectTranscript(state, sends).rows;
        for (const echo of entry.rows.filter((row) => row.timelineMarker?.kind === "queued")) {
          const send = sends.find((candidate) => candidate.content === echo.content);
          const was = before.findIndex((row) => row.id === send?.id);
          const now = after.findIndex((row) => row.content === echo.content);
          eq(now, was, `the echo of "${echo.content}" takes the pending row's exact place`);
          eq(after[now]?.id !== send?.id, true, `the echo of "${echo.content}" replaces the send's identity`);
        }
        sends = recordEchoes(sends, state);
        steps.push(after.map(label));
      }
      for (const send of SENT_AFTER_LINE.get(index) ?? []) {
        sends = [...sends, send];
        steps.push(projectTranscript(state, sends).rows.map(label));
      }
    });
    const final = projectTranscript(state, sends);
    eq(final.rows.map(label), EXPECTED, "sends interleave with the replies they triggered, not file arrival order");
    eq(
      steps.every((step) => isSubsequence(step, EXPECTED)),
      true,
      "no row ever changes place relative to another across the live replay",
    );
    eq(final.pending.length, 0, "every send reconciles with its queued echo");
    eq(final.rows.filter((row) => row.type === "user").length, 4, "no user row duplicates");
    eq(new Set(final.rows.map((row) => row.id)).size, final.rows.length, "every projected row keeps a unique identity");
    eq(sends.map((send) => send.sentAtMs), SENDS.map((send) => send.sentAtMs), "send keys are captured once and never replaced");
    eq(new Set(sends.map((send) => send.sentAtMs)).size, sends.length, "each send owns a distinct key");
  }

  // ── Hydration: the same bytes through away/back, remount and restart.
  {
    const hydrate = () => reduceTranscript(createTranscriptState(), {
      kind: "snapshot",
      authority: SESSION,
      range: { start: 0, end: offset },
      rows: entries.flatMap((entry) => entry.rows),
    }).state;
    const committed = hydrate();
    eq(
      transcriptRows(committed).map(label).join("|") === EXPECTED.join("|"),
      false,
      "file order alone misplaces the late queued echoes",
    );
    const recorded = recordEchoes(SENDS, committed);
    eq(recorded.every((send) => send.echo != null), true, "every send records its echo identity");
    const awayBack = projectedRows(hydrate(), recorded);
    const remount = projectedRows(hydrate(), recorded);
    const restored = JSON.parse(JSON.stringify(recorded)) as ChatSend[];
    const restart = projectedRows(hydrate(), restored);
    eq(projectTranscript(hydrate(), recorded).rows.map(label), EXPECTED, "hydration projects the live order");
    eq(JSON.stringify(remount), JSON.stringify(awayBack), "remount hydration is byte-for-byte identical");
    eq(JSON.stringify(restart), JSON.stringify(awayBack), "restart hydration from stored sends is byte-for-byte identical");
    eq(
      projectTranscript(hydrate()).rows.map(label),
      EXPECTED,
      "without stored sends the echoes keep their provider enqueue time",
    );
  }

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});

/**
 * Claude sub-agent calls project the neutral agent capability, keyed by the launching call.
 * Run: npx tsx plugins/provider-claude/tests/subagents.test.ts
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import { fileURLToPath } from "node:url";

interface Block { kind: string; data: Record<string, unknown> }
interface Row { role: string; blocks: Block[] }
interface Plugin {
  parseSessionDelta(chunk: string): { messages: Row[] };
  sessionUsageSources(sessionFilePath: string, sessionId: string): string[];
}

const SID = "f44205ef-4a6f-4090-9506-1978b4eada1b";
const AID = "af66d7d8e4a99a6bb";
const CALL = "toolu_014GV5zWvScrS4JGfgwQCbCq";
const FOREIGN_SID = "00000000-0000-0000-0000-000000000000";
const projects = fileURLToPath(new URL("./fixtures/subagents/projects", import.meta.url)).replace(/\\/g, "/");
const projectDir = `${projects}/C--Users-User-AppData-Local-Temp-snake-demo`;

const tailReads: string[] = [];
let hideSidechain = false;

(globalThis as { host?: unknown }).host = {
  manifestJson: () => "null",
  log: () => undefined,
  platform: () => "linux",
  expandHome: (path: string) => path === "~/.claude/projects" ? projects : path,
  fs: {
    readFileTail(path: string, maxBytes: number): string | null {
      tailReads.push(path);
      if (hideSidechain || !fs.existsSync(path)) return null;
      const raw = fs.readFileSync(path, "utf8");
      return raw.slice(Math.max(0, raw.length - maxBytes));
    },
    readDir(path: string) {
      if (!fs.existsSync(path)) return [];
      return fs.readdirSync(path, { withFileTypes: true }).map((entry) => ({
        name: entry.name, path: `${path}/${entry.name}`, isFile: entry.isFile(), isDir: entry.isDirectory(),
        size: 0, modifiedMs: null, createdMs: null,
      }));
    },
  },
};
const plugin = (await import("../src/plugin")).default as unknown as Plugin;

const lines = fs.readFileSync(`${projectDir}/${SID}.jsonl`, "utf8").split("\n").filter(Boolean);
const [callLine, launchLine, queueLine, noticeLine] = lines;

function parse(chunk: string): Row[] {
  return JSON.parse(JSON.stringify(plugin.parseSessionDelta(chunk))).messages;
}

function agentBlocks(rows: Row[]): Block[] {
  return rows.flatMap((row) => row.blocks.filter((block) => block.data.tool_kind === "agent"));
}

function onlyAgentInput(rows: Row[]): Record<string, unknown> {
  const blocks = agentBlocks(rows);
  assert.equal(blocks.length, 1, "exactly one agent record");
  assert.equal(blocks[0].data.tool_flow_id, CALL, "every record of the launch folds under its call id");
  return blocks[0].data.input as Record<string, unknown>;
}

// The Agent tool_use projects a sub-agent start.
assert.deepEqual(onlyAgentInput(parse(callLine)), {
  agent_call_id: CALL, agent_state: "started", agent_description: "Find all enums in snake-demo", agent_model: "Haiku",
});

// The async launch result names the agent and replaces the raw tool result.
const launchRows = parse(launchLine);
assert.equal(launchRows.length, 1);
assert.equal(launchRows[0].role, "assistant", "sub-agent progress is not user input");
assert.ok(!launchRows[0].blocks.some((block) => block.kind === "tool_result"), "the launch metadata result does not render separately");
assert.deepEqual(onlyAgentInput(launchRows), {
  agent_call_id: CALL, agent_id: AID, agent_state: "running", agent_description: "Find all enums in snake-demo", agent_model: "Haiku",
});

assert.equal(parse(queueLine).length, 0, "the queue-operation copy of the notification never renders");

// The notification completes the sub-agent with its reported usage and its own sidechain actions.
tailReads.length = 0;
const noticeRows = parse(noticeLine);
assert.equal(noticeRows.length, 1);
assert.equal(noticeRows[0].role, "assistant");
assert.ok(!noticeRows.some((row) => row.role === "status"), "no generic task marker for a sub-agent notification");
const notice = onlyAgentInput(noticeRows);
assert.equal(notice.agent_call_id, CALL);
assert.equal(notice.agent_id, AID);
assert.equal(notice.agent_state, "completed");
assert.equal(notice.agent_tokens, 40881);
assert.equal(notice.agent_tool_uses, 3);
assert.equal(notice.agent_duration_ms, 8141);
assert.equal(notice.agent_model, "Haiku");
assert.ok(String(notice.agent_result).startsWith("**Enums found in snake-demo:**"));
assert.deepEqual(tailReads, [`${projectDir}/${SID}/subagents/agent-${AID}.jsonl`], "one bounded read of this session's own sidechain");
const actions = notice.agent_actions as Array<Record<string, unknown>>;
assert.deepEqual(actions.map((action) => [action.kind, action.status]), [["command", "success"], ["read", "success"], ["read", "success"]]);
assert.equal(actions[0].description, "List files in the snake-demo directory");
assert.ok(String(actions[1].summary).endsWith("snake.js"));
assert.ok(String(actions[2].summary).endsWith("check.js"));

// The whole transcript yields one launch, never a status row.
const all = parse(`${lines.join("\n")}\n`);
assert.equal(all.filter((row) => row.role === "status").length, 0);
assert.deepEqual([...new Set(agentBlocks(all).map((block) => block.data.tool_flow_id))], [CALL]);

// A resumed agent notifies again under the same call and agent.
const resumed = noticeLine
  .replace("<subagent_tokens>40881</subagent_tokens>", "<subagent_tokens>52000</subagent_tokens>")
  .replace("<tool_uses>3</tool_uses>", "<tool_uses>5</tool_uses>");
const twice = agentBlocks(parse(`${noticeLine}\n${resumed}`));
assert.equal(twice.length, 2);
assert.deepEqual(twice.map((block) => (block.data.input as Record<string, unknown>).agent_call_id), [CALL, CALL]);
assert.deepEqual(twice.map((block) => (block.data.input as Record<string, unknown>).agent_id), [AID, AID]);
assert.equal((twice[1].data.input as Record<string, unknown>).agent_tokens, 52000);
assert.equal((twice[1].data.input as Record<string, unknown>).agent_tool_uses, 5);

// A missing sidechain degrades to notification data only.
hideSidechain = true;
const bare = onlyAgentInput(parse(noticeLine));
hideSidechain = false;
assert.equal(bare.agent_state, "completed");
assert.equal(bare.agent_tokens, 40881);
assert.equal(bare.agent_actions, undefined);
assert.equal(bare.agent_model, undefined);
assert.ok(bare.agent_result);

// A row naming another session never reads this session's sidechain.
tailReads.length = 0;
const foreign = onlyAgentInput(parse(noticeLine.replace(`"sessionId":"${SID}"`, `"sessionId":"${FOREIGN_SID}"`)));
assert.equal(foreign.agent_actions, undefined);
assert.ok(tailReads.every((path) => path.includes(FOREIGN_SID) && !path.includes(SID)));

// A background command notification keeps the generic marker.
const background = JSON.stringify({ type: "user", uuid: "bg-1", message: { role: "user", content:
  "<task-notification>\n<task-id>bawoofxfm</task-id>\n<tool-use-id>toolu_background</tool-use-id>\n<status>completed</status>\n<summary>done</summary>\n</task-notification>" } });
const backgroundRows = parse(background);
assert.equal(backgroundRows.length, 1);
assert.equal(backgroundRows[0].role, "status");
assert.equal(agentBlocks(backgroundRows).length, 0);

// Usage sources: only this session's agent logs, never another session's.
assert.deepEqual(plugin.sessionUsageSources(`${projectDir}/${SID}.jsonl`, SID), [`${projectDir}/${SID}/subagents/agent-${AID}.jsonl`]);
assert.deepEqual(plugin.sessionUsageSources(`${projectDir}/${SID}.jsonl`, FOREIGN_SID), [], "a file not named for the session yields nothing");
assert.deepEqual(plugin.sessionUsageSources(`${projectDir}/${FOREIGN_SID}.jsonl`, FOREIGN_SID), [], "an absent subagents dir yields nothing");

console.log("provider-claude subagents: ok");

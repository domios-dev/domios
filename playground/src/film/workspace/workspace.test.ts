import assert from "node:assert/strict";
import test from "node:test";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { beatFrame, compileBeat } from "./beat";
import { STREAM_BURST, STREAM_GAP, streamBursts, TOOL_SPAN, TYPE_CHAR, typingTimes } from "./cadence";
import { anyRunning, IDLE, localTime, pauseClocks, resumeClocks, startClock } from "./clocks";
import { focusTab, goPlace, initialNav, openTask, openView, placeOf, selectMachine, selectWorkspace, type World } from "./nav";

const TEXT = "Run the 2.4 release gate and get sync QA green on gpu-rig.";

test("typing moves at a person's speed with pauses after punctuation", () => {
  const times = typingTimes(TEXT, 7);
  assert.equal(times.length, TEXT.length);
  const gaps = times.slice(1).map((t, i) => t - times[i]);
  for (const [i, gap] of gaps.entries()) {
    assert.ok(gap >= TYPE_CHAR[0] - 1e-9, `gap ${i} too fast: ${gap}`);
    if (!/[.,;:?! ]/.test(TEXT[i])) assert.ok(gap <= TYPE_CHAR[1] + 1e-9, `gap ${i} too slow: ${gap}`);
  }
  const afterComma = TEXT.indexOf(".", 5);
  assert.ok(gaps[afterComma] >= 0.25, "a pause follows punctuation");
  assert.deepEqual(typingTimes(TEXT, 7), times, "the same seed plays the same script");
});

test("agent text streams in 3-12 character bursts every 30-60 ms", () => {
  const bursts = streamBursts(TEXT.repeat(4), 3);
  assert.equal(bursts[bursts.length - 1].end, TEXT.length * 4);
  for (let i = 1; i < bursts.length; i++) {
    const size = bursts[i].end - bursts[i - 1].end;
    const gap = bursts[i].at - bursts[i - 1].at;
    if (i < bursts.length - 1) assert.ok(size >= STREAM_BURST[0] && size <= STREAM_BURST[1], `burst ${i} is ${size} chars`);
    assert.ok(gap >= STREAM_GAP[0] - 1e-9 && gap <= STREAM_GAP[1] + 1e-9, `gap ${i} is ${gap}`);
  }
});

const last = <T,>(list: T[]) => list[list.length - 1];

const call: NormalizedChatMessage = { id: "run", type: "tool_call", content: "", toolName: "Bash", toolId: "run-call", toolKind: "command", toolSummary: "npm test" };

test("a beat types, sends, runs a tool for a measured time, streams a reply, then holds", () => {
  const beat = compileBeat([], [
    { kind: "type", id: "ask", text: "Run the tests." },
    { kind: "tool", call, result: "14 passed", seconds: 9 },
    { kind: "say", id: "reply", text: "All 14 pass. Nothing else changed in the lane." },
  ], 1);
  const typing = beatFrame(beat, beat.steps[0].at + 0.3);
  assert.ok(typing.draft.length > 0 && typing.draft.length < "Run the tests.".length);
  assert.equal(typing.messages.length, 0);
  assert.equal(typing.working, false);

  const tool = beat.steps[1];
  const running = beatFrame(beat, tool.at + 0.1);
  assert.equal(running.draft, "");
  assert.equal(running.working, true);
  assert.equal(last(running.messages)?.toolStatus, "running");
  assert.ok(tool.kind === "tool" && tool.done - tool.at === TOOL_SPAN[1], "a long tool shows at most 2.5 s of running");

  const done = beatFrame(beat, tool.kind === "tool" ? tool.done + 0.01 : 0);
  const finished = done.messages.find((m) => m.id === "run");
  assert.equal(finished?.toolStatus, "completed");
  assert.equal(finished?.toolDurationMs, 2500);
  assert.equal(last(done.messages)?.type, "tool_result");

  const streaming = beatFrame(beat, beat.steps[2].at + 0.04);
  assert.equal(streaming.streaming, "reply");
  assert.ok((last(streaming.messages)?.content.length ?? 0) < 20);

  const held = beatFrame(beat, beat.end + 60);
  assert.deepEqual(held, beatFrame(beat, beat.end), "the last frame holds");
  assert.equal(held.working, false);
  assert.equal(held.streaming, null);
});

test("an until step lines a beat up with the story clock", () => {
  const beat = compileBeat([], [{ kind: "until", at: 12 }, { kind: "say", id: "late", text: "Report arrived." }], 2);
  assert.equal(beat.steps[0].at, 12);
});

type M = "local" | "laptop" | "rig";
const WORLD: World<M> = {
  machines: [
    { id: "local", home: "helm", workspaces: [{ id: "ws-app", tabs: ["helm", "atlas", "mira"] }, { id: "ws-site", tabs: ["juno", "vale"] }] },
    { id: "laptop", home: "sable", workspaces: [{ id: "ws-laptop", tabs: ["sable"] }] },
    { id: "rig", home: "orbit", workspaces: [{ id: "ws-rig", tabs: ["orbit"] }] },
  ],
  local: "local",
  remote: "rig",
  teams: { machine: "local", workspace: "ws-site", tab: "juno" },
};

test("every chip lands on its place and keeps what the visitor opened", () => {
  const start = initialNav(WORLD);
  assert.equal(placeOf(WORLD, start), "chat");
  for (const place of ["chat", "plan", "machines", "teams", "usage", "plugins"] as const) assert.equal(placeOf(WORLD, goPlace(WORLD, start, place)), place);
  const onMira = focusTab(WORLD, start, "mira");
  const back = goPlace(WORLD, goPlace(WORLD, onMira, "plan"), "chat");
  assert.equal(back.tab["ws-app"], "mira", "returning to chat keeps the open tab");
  assert.equal(goPlace(WORLD, onMira, "chat"), onMira, "a chip for the current place changes nothing");
  const fromTeams = goPlace(WORLD, goPlace(WORLD, start, "teams"), "chat");
  assert.equal(fromTeams.workspace.local, "ws-app", "chat leaves the teams workspace");
});

test("the navigation map: machine rows, a mesh report, task links, owner chips and team rows", () => {
  const start = initialNav(WORLD);
  const rig = selectMachine(start, "rig");
  assert.equal(placeOf(WORLD, rig), "machines");
  assert.equal(rig.tab[rig.workspace.rig], "orbit", "a machine shows its own tabs");
  assert.equal(selectMachine(openTask(start, null), "rig").view, "plan", "switching machines keeps the view");

  const report = focusTab(WORLD, start, "orbit");
  assert.equal(report.machine, "rig", "the mesh report opens the sender's machine");
  assert.equal(placeOf(WORLD, report), "machines");

  const card = openTask(start, "task_fn203r");
  assert.equal(placeOf(WORLD, card), "plan");
  assert.equal(card.task, "task_fn203r");

  const owner = focusTab(WORLD, card, "mira");
  assert.equal(placeOf(WORLD, owner), "chat");
  assert.equal(owner.task, null, "an owner chip leaves the board for that agent's chat");

  const teams = goPlace(WORLD, start, "teams");
  assert.equal(placeOf(WORLD, selectWorkspace(teams, "ws-app")), "teams", "browsing workspaces stays on the teams view");
  const team = focusTab(WORLD, teams, "juno");
  assert.equal(placeOf(WORLD, team), "chat", "a team's agent opens its chat");
  assert.equal(team.workspace.local, "ws-site");
  assert.equal(focusTab(WORLD, start, "nobody"), start, "an unmapped click changes nothing");
});

test("app views from the sidebar report their place, and a chip leaves them", () => {
  const start = initialNav(WORLD);
  const jobs = openView(start, "jobs");
  assert.equal(placeOf(WORLD, jobs), "chat");
  assert.equal(goPlace(WORLD, jobs, "chat").view, "chat", "the chat chip leaves Jobs");
  assert.equal(placeOf(WORLD, openView(start, "mesh")), "machines", "the Mesh view belongs to machines");
  const onLaptop = selectMachine(start, "laptop");
  assert.equal(goPlace(WORLD, onLaptop, "machines"), onLaptop, "a machines chip keeps the machine the visitor picked");
});

test("a beat never started reads as finished; a started one runs, holds still while paused, and resumes where it was", () => {
  assert.equal(localTime(IDLE, "helm", 5_000), Infinity);
  const started = startClock(IDLE, "helm", 1_000);
  assert.equal(localTime(started, "helm", 3_000), 2);
  assert.equal(anyRunning(started, { helm: 10 }, 3_000), true);
  const paused = pauseClocks(started, 3_000);
  assert.equal(localTime(paused, "helm", 60_000), 2, "a pause freezes the beat");
  assert.equal(anyRunning(paused, { helm: 10 }, 60_000), false);
  const resumed = resumeClocks(paused, 60_000);
  assert.equal(localTime(resumed, "helm", 61_000), 3, "no jump on resume");
  assert.equal(anyRunning(resumed, { helm: 10 }, 80_000), false, "a finished beat holds");
});

test("one tour loop runs 25-35 s at a human pace and visits the places L15.4 names", async () => {
  const { dwellBefore, DWELL, HOP, hopDuration, loopLength, tourRoute } = await import("./tour");
  const route = tourRoute({ agent: "mira", home: "helm", card: "FN-203" });
  const slowest = loopLength(route, route.map(() => HOP[1]));
  const fastest = loopLength(route, route.map(() => HOP[0]));
  assert.ok(fastest >= 25_000 && slowest <= 35_000, `loop is ${fastest}-${slowest} ms`);
  for (const [i] of route.entries()) assert.ok(dwellBefore(i) >= DWELL[0] && dwellBefore(i) <= DWELL[1]);
  assert.ok(hopDuration(0, 0) >= HOP[0] && hopDuration(3000, 3000) <= HOP[1]);
  const notes = route.map((step) => step.note).join(" | ");
  for (const visit of ["agent's chat", "gpu-rig", "card", "Plugins", "Memory", "back"]) assert.ok(notes.includes(visit), visit);
});

test("sample activity appends across the loop seam and stays bounded over long visits", async () => {
  const { activityFrame, ACTIVITY_HISTORY_LIMIT } = await import("./activity");
  const { compileBeat } = await import("./beat");
  const scripts = ["first task", "second task"].map((content) => ({
    duration: 8,
    beats: { demo: compileBeat([], [{ kind: "type", id: "ask", text: content }], 4) },
  }));
  assert.deepEqual(activityFrame(scripts, "demo", 0).messages, [], "the first frame does not invent past rounds");
  const before = activityFrame(scripts, "demo", 7.99);
  const after = activityFrame(scripts, "demo", 8.01);
  assert.equal(before.messages[before.messages.length - 1].id, after.messages[after.messages.length - 1].id);
  const later = activityFrame(scripts, "demo", 608);
  assert.ok(later.messages.length <= ACTIVITY_HISTORY_LIMIT);
  assert.equal(later.messages.filter((message) => message.content === "first task").length, 1, "a repeated episode does not stack identical messages");
  assert.equal(new Set(later.messages.map((message) => message.id)).size, later.messages.length);
  assert.ok(later.messages.some((message) => message.content === "first task"));
  assert.ok(later.messages.some((message) => message.content === "second task"));
});

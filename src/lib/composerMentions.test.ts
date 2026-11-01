import assert from "node:assert/strict";
import { activeMentionQuery, applyComposerMention, compareMentions, createMentionRegistry, mentionDismissed, mentionMatch, type ComposerMention, type MentionContext } from "./composerMentions";
import { tabMentionSource } from "./mentionSources/tabs";
import { taskMentionSource } from "./mentionSources/tasks";
import { planMentionSource } from "./mentionSources/plans";
import { blueprintMentionSource } from "./mentionSources/blueprints";
import { fileMentionSource } from "./mentionSources/files";
import { projectIdentity } from "./mentionSources/scope";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../components/Canvas/cards/fixtures.testdata";
import { fixtureGraph, fixtureNode } from "../components/Automation/fixtures.testdata";
import type { Plan, Task } from "../components/Canvas/types";
import type { Tab, Workspace } from "./types";

const ctx: MentionContext = { scope: { tabId: "self", workspaceId: "ws1", projectPath: "/project1", teamId: "team1" }, signal: new AbortController().signal };
const row = (label: string, extra: Partial<ComposerMention> = {}): ComposerMention => ({ id: label, label, description: "", insertText: label, group: "Fixture", match: 1, ...extra });
const task = (key: string, status: Task["status"], name = "Compose"): Task => ({ ...FIXTURE_TASK, id: `task_${key}`, key, status, name });
const plan = (id: string, team: string, project: string, tasks: Task[]): Plan => ({ ...FIXTURE_PLAN, id, title: id, team_id: team, project_path: project, status: "active", mode: "execution", tasks });
const plans = [plan("pl_a", "team1", "/project1", [task("COD-1", "done"), task("COD-2", "pending"), task("COD-3", "in_progress", "Complete composer"), task("COD-4", "cancelled"), task("COD-5", "in_review"), task("COD-6", "blocked")]),
  plan("pl_b", "team2", "/project1", [task("COD-7", "done")]), { ...plan("pl_solo", "", "/project1", [task("COD-9", "done")]), team_id: null }, plan("pl_c", "team3", "/project2", [task("COD-8", "done")]),
  { ...plan("pl_archived", "team1", "/project1", []), status: "archived" as const },
  { ...plan("pl_closed", "team1", "/project1", []), mode: "complete" as const }];
const tab = (id: string, team: string | null, ws: string, title: string, agent = true): Tab => ({ id, team_id: team, workspace_id: ws, title, description: "Compose code", agent_type: agent ? "fixture" : null, tab_type: "Terminal", pty_id: null, cwd: null });
const tabs = { self: tab("self", "team1", "ws1", "Composer"), peer: tab("peer", "team2", "ws1", "Compose"), far: tab("far", null, "ws2", "Comet"), shell: tab("shell", "team1", "ws1", "Compose", false) };
const workspaces = [{ id: "ws1", tabs: ["self", "peer", "shell"], name: "One" }, { id: "ws2", tabs: ["far"], name: "Two" }] as Workspace[];
const tests: Array<[string, () => void | Promise<void>]> = [];
const test = (name: string, fn: () => void | Promise<void>) => tests.push([name, fn]);
test("source presentation metadata carries typed kind and provider identity", async () => {
  const tabRows = await tabMentionSource(() => ({ tabs, workspaces })).search("", ctx);
  assert.equal(tabRows.find(row => row.id === "tabs:self")?.providerId, "fixture");
  assert.equal(tabRows.find(row => row.id === "tabs:shell")?.providerId, null);
  assert.ok(tabRows.every(row => row.kind === "tab"));
  const tasks = await taskMentionSource(() => plans).search("COD-3", ctx);
  assert.equal(tasks.find(row => row.label === "COD-3")?.kind, "task");
  assert.equal(tasks.find(row => row.label === "COD-3")?.status, "in_progress");
  assert.ok((await planMentionSource(() => plans).search("", ctx)).every(row => row.kind === "plan"));
  const files = await fileMentionSource(async () => [{ name: "readme", path: "/project1/readme" }]).search("", ctx);
  assert.equal(files[0].kind, "file");
});
test("project identity preserves case-sensitive project boundaries", () => {
  assert.notEqual(projectIdentity("/project/A"), projectIdentity("/project/a"));
  assert.equal(projectIdentity("C:\\Project\\Repo\\"), projectIdentity("c:/project/repo"));
  assert.equal(projectIdentity("\\\\?\\C:\\Project\\Repo"), projectIdentity("c:/project/repo"));
  assert.equal(projectIdentity("\\\\?\\UNC\\server\\share"), projectIdentity("//server/share"));
});
test("caret query spans a token in the middle and rejects email/selection", () => {
  assert.deepEqual(activeMentionQuery("hi @compose later", 7), { start: 3, end: 11, partial: "com" });
  assert.equal(activeMentionQuery("mail@name"), null);
  assert.equal(activeMentionQuery("@tab", 2, 3), null);
  assert.equal(activeMentionQuery("@tab next"), null);
  assert.deepEqual(activeMentionQuery("@"), { start: 0, end: 1, partial: "" });
});
test("insertion preserves both sides of the draft", () => {
  assert.equal(applyComposerMention("hi @compose later", activeMentionQuery("hi @compose later", 7)!, { insertText: "COD-12" }), "hi COD-12  later");
});
test("match quality is exact then prefix then subsequence", () => {
  assert.deepEqual([mentionMatch("cod", ["COD"]), mentionMatch("cod", ["Coding"]), mentionMatch("cd", ["Coding"]), mentionMatch("zz", ["Coding"])], [0, 1, 2, Infinity]);
});
test("file suggestion paths hide Windows extended prefixes", async () => {
  const source = fileMentionSource(async () => [
    { name: "a.ts", path: "\\\\?\\C:\\repo\\a.ts" },
    { name: "b.ts", path: "\\\\?\\UNC\\host\\share\\b.ts" },
  ]);
  const rows = await source.search("", ctx);
  assert.deepEqual(rows.map(row => row.description), ["C:\\repo\\a.ts", "\\\\host\\share\\b.ts"]);
});
test("extended Windows paths expose scoped Tasks and Plans alongside cwd Files", async () => {
  const bound = { ...plans[0], project_path: "\\\\?\\C:\\Project\\Repo", tasks: [task("COD-1", "done", "Water the basil"), task("COD-2", "pending", "Water the basil")] };
  const windows = { ...ctx, scope: { ...ctx.scope, projectPath: "c:/project/repo", cwd: "c:/project/repo/lane" } };
  const calls: string[] = [];
  const registry = createMentionRegistry([taskMentionSource(() => [bound]), planMentionSource(() => [bound]), fileMentionSource(async path => {
    calls.push(path); return [{ name: "cdp.mjs", path: `${path}/cdp.mjs` }];
  })]);
  assert.deepEqual((await registry.search("water", windows)).map(row => row.label), ["COD-2", "COD-1"]);
  assert.equal((await registry.search("pl_a", windows))[0].group, "Plans");
  assert.equal((await registry.search("cdp", windows))[0].group, "Files");
  assert.deepEqual(calls, Array(3).fill("c:/project/repo/lane"));
  assert.equal((await registry.search("cdp", { ...ctx, scope: { ...ctx.scope, workspaceId: null } })).length, 0);
});
test("proximity then agents outrank match quality except exact title/id", async () => {
  const source = tabMentionSource(() => ({ tabs, workspaces }));
  const registry = createMentionRegistry([source]);
  assert.deepEqual((await registry.search("co", ctx)).map((r) => r.id), ["tabs:self", "tabs:shell", "tabs:peer", "tabs:far"]);
  assert.equal((await registry.search("Comet", ctx))[0].id, "tabs:far");
  assert.equal((await registry.search("peer", ctx))[0].id, "tabs:peer");
});
test("same workspace precedes others for non-Team callers and mesh is labelled", async () => {
  const mesh = { ...tabs.far, id: "mesh:peer:far", remote_peer_id: "peer", remote_peer_name: "Remote" };
  const result = await createMentionRegistry([tabMentionSource(() => ({ tabs: { ...tabs, mesh }, workspaces }))]).search("co", { ...ctx, scope: { ...ctx.scope, teamId: null } });
  assert.equal(result[result.length - 1]?.group, "Tabs · Mesh");
  assert.equal(result[0].group, "Tabs · Other Teams");
});
test("workspace and Other Teams groups remain contiguous across agents and shells", async () => {
  const mixed = { alpha: tab("alpha", null, "ws1", "Alpha"), beta: tab("beta", "team2", "ws1", "Beta"), gamma: tab("gamma", null, "ws1", "Gamma", false) };
  const result = await createMentionRegistry([tabMentionSource(() => ({ tabs: mixed, workspaces }))]).search("", ctx);
  assert.deepEqual(result.map(row => row.label), ["Alpha", "Gamma", "Beta"]);
  const headings = result.filter((row, index) => row.group !== result[index - 1]?.group).map(row => row.group);
  assert.deepEqual(headings, ["Tabs · Workspace", "Tabs · Other Teams"]);
  assert.equal(new Set(headings).size, headings.length);
});
test("Team task scope never leaks another Team or project, including exact keys", async () => {
  const registry = createMentionRegistry([taskMentionSource(() => plans)]);
  assert.deepEqual((await registry.search("", ctx)).map((r) => r.label), ["COD-3", "COD-5", "COD-6", "COD-2", "COD-1", "COD-4"]);
  assert.equal((await registry.search("COD-7", ctx)).length, 0);
  assert.equal((await registry.search("COD-8", ctx)).length, 0);
});
test("task status precedes text quality and exact key overrides status", async () => {
  const registry = createMentionRegistry([taskMentionSource(() => plans)]);
  assert.equal((await registry.search("Compose", ctx))[0].label, "COD-3");
  assert.equal((await registry.search("COD-1", ctx))[0].label, "COD-1");
  assert.ok(compareMentions(row("busy", { match: 2, statusOrder: 0 }), row("done", { match: 0, statusOrder: 5 })) < 0);
});
test("non-Team tasks and plans stay in current project; closed plans are excluded", async () => {
  const registry = createMentionRegistry([planMentionSource(() => plans), taskMentionSource(() => plans)]);
  const caller = { ...ctx, scope: { ...ctx.scope, teamId: null } };
  assert.equal((await registry.search("COD-7", caller)).length, 0);
  assert.equal((await registry.search("COD-9", caller))[0].label, "COD-9");
  assert.equal((await registry.search("COD-8", caller)).length, 0);
  assert.deepEqual((await createMentionRegistry([planMentionSource(() => plans)]).search("pl", ctx)).map((r) => r.label), ["pl_a"]);
});
test("Blueprint scope excludes other Teams even when owned through a tab", async () => {
  const graph = { ...fixtureGraph(), nodes: [
    fixtureNode({ kind: "blueprint", id: "own", workspace_id: "ws1", team_id: "team1", owner_tab_id: null }, "Own"),
    fixtureNode({ kind: "blueprint", id: "other", workspace_id: "ws1", team_id: "team2", owner_tab_id: null }, "Other"),
    fixtureNode({ kind: "blueprint", id: "via-tab", workspace_id: "ws1", team_id: null, owner_tab_id: "peer" }, "Other tab"),
    fixtureNode({ kind: "blueprint", id: "workspace", workspace_id: "ws1", team_id: null, owner_tab_id: null }, "Workspace"),
    fixtureNode({ kind: "blueprint", id: "project2", workspace_id: "ws2", team_id: null, owner_tab_id: null }, "Far"),
  ] };
  const registry = createMentionRegistry([blueprintMentionSource(async () => graph, () => tabs)]);
  assert.deepEqual((await registry.search("", ctx)).map((r) => r.label), ["Own", "Workspace"]);
  assert.deepEqual((await registry.search("", { ...ctx, scope: { ...ctx.scope, teamId: null } })).map((r) => r.label), ["Workspace"]);
});
test("registry accepts foreign modules, rejects duplicate ids and cancels stale work", async () => {
  let complete!: (rows: ComposerMention[]) => void;
  const source = { id: "custom", group: "Custom", search: () => new Promise<ComposerMention[]>((resolve) => { complete = resolve; }) };
  assert.throws(() => createMentionRegistry([source, source]), /Duplicate/);
  const controller = new AbortController();
  const pending = createMentionRegistry([source]).search("", { ...ctx, signal: controller.signal });
  controller.abort(); complete([row("old")]);
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal((await createMentionRegistry([taskMentionSource(() => plans)]).search("ds-tasks", ctx)).length, 0);
  const ranked = await createMentionRegistry([
    { id: "names", group: "Names", search: async () => [row("prefix")] },
    { id: "keys", group: "Keys", search: async () => [row("COD-9", { exact: true })] },
  ]).search("COD-9", ctx);
  assert.equal(ranked[0].label, "COD-9");
});
test("dismissal survives typing, reopens on backspace or a fresh @", () => {
  const dismissed = { start: 0, draft: "@co", caret: 3 };
  assert.equal(mentionDismissed(dismissed, "@cod", 4, activeMentionQuery("@cod")!), true);
  assert.equal(mentionDismissed(dismissed, "@c", 2, activeMentionQuery("@c")!), false);
  assert.equal(mentionDismissed(dismissed, "@co @", 5, activeMentionQuery("@co @")!), false);
});
async function main() { for (const [name, fn] of tests) { await fn(); console.log(`PASS ${name}`); } console.log(`${tests.length} tests passed`); }
void main().catch((error) => { console.error(error); process.exitCode = 1; });

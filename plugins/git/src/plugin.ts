// Git plugin — CodeTerm's local-git presence, authored in TypeScript against
// @codeterm/plugin-sdk and compiled to QuickJS-compatible plugin.js by
// scripts/build-plugin.mjs. Contributes a pane-footer status bubble, a glance
// popover, and the full Git view ops the iframe UI drives via window.ct.invoke.
import type {
  ExecOpts,
  ExecResult,
  GlanceView,
  PaneContext,
  PluginModule,
  StatusBubble,
  ViewNode,
} from "@codeterm/plugin-sdk";

function exec(opts: ExecOpts): ExecResult {
  const raw = host.exec(JSON.stringify(opts));
  try {
    return JSON.parse(raw) as ExecResult;
  } catch (e) {
    return { error: String(e) };
  }
}

function git(cwd: string, args: string[]): ExecResult {
  return exec({ bin: "git", args: ["-C", cwd].concat(args) });
}

// --- pure parsers (exported for tests) ---

interface Porcelain {
  total: number;
  staged: number;
  unstaged: number;
  untracked: number;
}

// `git status --porcelain` → { total, staged, unstaged, untracked }.
function parsePorcelain(out: string | undefined): Porcelain {
  const lines = (out || "").split("\n").filter((l) => l.length > 0);
  let staged = 0;
  let unstaged = 0;
  let untracked = 0;
  lines.forEach((l) => {
    const x = l.charAt(0);
    const y = l.charAt(1);
    if (x === "?" && y === "?") {
      untracked++;
      return;
    }
    if (x !== " " && x !== "?") staged++;
    if (y !== " " && y !== "?") unstaged++;
  });
  return { total: lines.length, staged, unstaged, untracked };
}

// `git log --oneline` → array of short lines (already one per commit).
function parseLog(out: string | undefined): string[] {
  return (out || "")
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
}

function branchOf(cwd: string): string | null {
  const r = git(cwd, ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (r.error || r.code !== 0) return null;
  return (r.stdout || "").trim() || null;
}

// --- capabilities ---

function statusBubble(ctx: PaneContext): StatusBubble | null {
  const cwd = ctx && ctx.cwd;
  if (!cwd) return null;
  const branch = branchOf(cwd);
  if (!branch) return null; // not a repo → no pill
  const d = parsePorcelain(git(cwd, ["status", "--porcelain"]).stdout);
  return {
    label: branch,
    tone: "default",
    icon: "git",
    tooltip: d.total ? `${d.total} changed — open Git` : "clean — open Git",
    // Click opens the plugin's own Git view (its sandboxed iframe UI).
    action: "openPanel:view:git",
  };
}

function renderGlance(ctx: PaneContext): GlanceView {
  const cwd = ctx && ctx.cwd;
  if (!cwd) {
    return {
      title: "Git",
      nodes: [{ kind: "text", text: "No working directory for this pane.", style: { tone: "muted" } }],
    };
  }
  const branch = branchOf(cwd);
  if (!branch) {
    return { title: "Git", nodes: [{ kind: "text", text: "Not a git repository.", style: { tone: "muted" } }] };
  }
  const d = parsePorcelain(git(cwd, ["status", "--porcelain"]).stdout);
  const commits = parseLog(git(cwd, ["log", "--oneline", "-n", "5"]).stdout);

  const nodes: ViewNode[] = [
    { kind: "keyVal", key: "Branch", value: branch },
    {
      kind: "keyVal",
      key: "Working tree",
      value: d.total ? `${d.total} changed (${d.staged} staged)` : "clean",
      style: { tone: d.total ? "warn" : "ok" },
    },
  ];
  if (commits.length) {
    nodes.push({ kind: "divider" });
    nodes.push({ kind: "text", text: "Recent commits", style: { tone: "muted" } });
    commits.forEach((c) => nodes.push({ kind: "text", text: c }));
  }
  nodes.push({ kind: "divider" });
  nodes.push({ kind: "button", label: "Open Git", action: "openPanel:view:git" });
  return { title: `Git · ${branch}`, nodes };
}

// --- view ops: faithful port of the former native git_routes.rs ---
// The plugin's iframe UI (ui/) calls these via window.ct.invoke(method, args).
// Every op shells `git -C <cwd>` and reproduces the exact REST output shapes.

const FS = "\x1f"; // field separator inside the graph pretty-format
const RS = "\x1e"; // record separator between commits

interface Ref {
  name: string;
  kind: "tag" | "head" | "branch" | "remote";
}

// `%D`-style ref decoration → [{ name, kind }].
function parseRefs(s: string | undefined): Ref[] {
  if (!s) return [];
  return s
    .split(", ")
    .map((raw): Ref | null => {
      raw = raw.trim();
      if (!raw) return null;
      if (raw.indexOf("tag: ") === 0) {
        let t = raw.slice(5);
        if (t.indexOf("refs/tags/") === 0) t = t.slice(10);
        return { name: t.replace(/\^\{\}$/, ""), kind: "tag" };
      }
      if (raw.indexOf("HEAD -> ") === 0) {
        let h = raw.slice(8);
        if (h.indexOf("refs/heads/") === 0) h = h.slice(11);
        return { name: h, kind: "head" };
      }
      if (raw === "HEAD") return { name: "HEAD", kind: "head" };
      if (raw.indexOf("refs/heads/") === 0) return { name: raw.slice(11), kind: "branch" };
      if (raw.indexOf("refs/remotes/") === 0) return { name: raw.slice(13), kind: "remote" };
      if (raw.indexOf("refs/tags/") === 0) return { name: raw.slice(10).replace(/\^\{\}$/, ""), kind: "tag" };
      return { name: raw, kind: "branch" };
    })
    .filter((r): r is Ref => r !== null);
}

interface GraphCommit {
  sha: string;
  parents: string[];
  author: string;
  date: string;
  refs: Ref[];
  subject: string;
}

function parseGraph(out: string | undefined): GraphCommit[] {
  return (out || "")
    .split(RS)
    .map((rec) => (rec.charAt(0) === "\n" ? rec.slice(1) : rec))
    .filter((rec) => rec.indexOf(FS) !== -1)
    .map((rec): GraphCommit => {
      const f = rec.split(FS); // [0]="" [1]sha [2]parents [3]author [4]date [5]refs [6]subject
      return {
        sha: f[1] || "",
        parents: (f[2] || "").split(" ").filter((p) => p.length > 0),
        author: f[3] || "",
        date: f[4] || "",
        refs: parseRefs(f[5] || ""),
        subject: f[6] || "",
      };
    })
    .filter((c) => c.sha.length > 0);
}

interface Rename {
  path: string;
  old_path: string | null;
}

// `old => new` and `dir/{old => new}/file` rename forms → { path, old_path }.
function splitRename(s: string): Rename {
  const brace = s.indexOf("{");
  if (brace !== -1) {
    const close = s.indexOf("}", brace);
    const arrow = brace !== -1 && close !== -1 ? s.slice(brace + 1, close).indexOf(" => ") : -1;
    if (close !== -1 && arrow !== -1) {
      const pre = s.slice(0, brace);
      const inner = s.slice(brace + 1, close);
      const post = s.slice(close + 1);
      const a = inner.indexOf(" => ");
      const oldMid = inner.slice(0, a);
      const newMid = inner.slice(a + 4);
      const collapse = (p: string) => p.replace(/\/\//g, "/");
      return { path: collapse(pre + newMid + post), old_path: collapse(pre + oldMid + post) };
    }
  }
  const top = s.indexOf(" => ");
  if (top !== -1) return { path: s.slice(top + 4), old_path: s.slice(0, top) };
  return { path: s, old_path: null };
}

interface NumstatFile {
  path: string;
  old_path: string | null;
  added: number | null;
  deleted: number | null;
  status: string;
}

// `git show --numstat` body → [{ path, old_path, added, deleted, status }].
function parseNumstat(out: string | undefined): NumstatFile[] {
  return (out || "")
    .split("\n")
    .map((line): NumstatFile | null => {
      if (!line) return null;
      const parts = line.split("\t");
      if (parts.length < 3) return null;
      const addS = parts[0];
      const delS = parts[1];
      const r = splitRename(parts.slice(2).join("\t"));
      const binary = addS === "-" && delS === "-";
      return {
        path: r.path,
        old_path: r.old_path,
        added: addS === "-" ? null : parseInt(addS, 10) || 0,
        deleted: delS === "-" ? null : parseInt(delS, 10) || 0,
        status: r.old_path ? "renamed" : binary ? "binary" : "modified",
      };
    })
    .filter((f): f is NumstatFile => f !== null);
}

function classifyPorcelain(x: string, y: string): string {
  if (x === "?" && y === "?") return "untracked";
  if (x === "U" || y === "U" || (x === "D" && y === "D") || (x === "A" && y === "A")) return "conflicted";
  if (x === "R" || y === "R") return "renamed";
  if (x === "D" || y === "D") return "deleted";
  if (x !== " " && x !== "?") return "staged";
  return "modified";
}

interface StatusEntry {
  path: string;
  old_path: string | null;
  status: string;
  staged: boolean;
}

// `git status --porcelain=v1 -z` → [{ path, old_path, status, staged }].
function parseStatusPorcelain(out: string | undefined): StatusEntry[] {
  const tokens = (out || "").split("\0").filter((t) => t.length > 0);
  const entries: StatusEntry[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const tok = tokens[i];
    if (tok.length < 3) continue;
    const x = tok.charAt(0);
    const y = tok.charAt(1);
    let oldPath: string | null = null;
    if (x === "R" || x === "C" || y === "R" || y === "C") {
      if (i + 1 < tokens.length) {
        oldPath = tokens[i + 1];
        i++;
      }
    }
    entries.push({
      path: tok.slice(3),
      old_path: oldPath,
      status: classifyPorcelain(x, y),
      staged: x !== " " && x !== "?",
    });
  }
  return entries;
}

type Counts = Record<string, [number | null, number | null]>;

// `git diff --numstat HEAD` → { path: [added, deleted] }.
function parseNumstatCounts(out: string | undefined): Counts {
  const map: Counts = {};
  (out || "").split("\n").forEach((line) => {
    if (!line) return;
    const parts = line.split("\t");
    if (parts.length < 3) return;
    const r = splitRename(parts.slice(2).join("\t"));
    map[r.path] = [
      parts[0] === "-" ? null : parseInt(parts[0], 10) || 0,
      parts[1] === "-" ? null : parseInt(parts[1], 10) || 0,
    ];
  });
  return map;
}

interface WorkdirFile extends StatusEntry {
  added: number | null;
  deleted: number | null;
}

function mergeWorkdirFiles(entries: StatusEntry[], counts: Counts): WorkdirFile[] {
  return entries.map((e) => {
    const c = e.status === "untracked" ? null : counts[e.path];
    return {
      path: e.path,
      old_path: e.old_path,
      status: e.status,
      staged: e.staged,
      added: c ? c[0] : null,
      deleted: c ? c[1] : null,
    };
  });
}

function baseName(p: string): string {
  const parts = String(p).replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || String(p);
}

function ok(r: ExecResult | undefined): boolean {
  return !!r && !r.error && r.code === 0;
}
function fail(r: ExecResult | undefined, msg: string): { error: string } {
  return { error: (r && (r.stderr || r.error)) || msg };
}

// --- ops ---

interface BranchStats {
  branch_base?: string;
  branch_added?: number;
  branch_deleted?: number;
}

function branchLineStats(cwd: string, branch: string | null): BranchStats {
  if (!branch) return {};
  let base: string | null = null;
  const up = git(cwd, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
  if (ok(up)) base = (up.stdout || "").trim() || null;
  if (!base) {
    ["origin/main", "origin/master", "main", "master"].some((cand) => {
      if (ok(git(cwd, ["rev-parse", "--verify", "-q", cand]))) {
        base = cand;
        return true;
      }
      return false;
    });
  }
  if (!base) return {};
  const mb = git(cwd, ["merge-base", base, "HEAD"]);
  if (!ok(mb)) return {};
  const baseSha = (mb.stdout || "").trim();
  if (!baseSha) return {};
  const ns = git(cwd, ["diff", "--numstat", baseSha, "HEAD"]);
  if (!ok(ns)) return {};
  let added = 0;
  let deleted = 0;
  (ns.stdout || "").split("\n").forEach((l) => {
    if (!l) return;
    const p = l.split("\t");
    if (p.length < 2) return;
    if (p[0] !== "-") added += parseInt(p[0], 10) || 0;
    if (p[1] !== "-") deleted += parseInt(p[1], 10) || 0;
  });
  return { branch_base: base, branch_added: added, branch_deleted: deleted };
}

interface GraphResult {
  commits: GraphCommit[];
  head_sha: string;
  branch: string | null;
  dirty: boolean;
  branch_base?: string;
  branch_added?: number;
  branch_deleted?: number;
}

function gitGraph(cwd: string, limit?: number, skip?: number): GraphResult | { error: string } {
  const r = git(cwd, [
    "-c", "core.quotePath=false", "log", "--branches", "--tags", "--remotes", "HEAD",
    "--topo-order", "--decorate=full",
    "--pretty=format:%x1f%H%x1f%P%x1f%an%x1f%aI%x1f%D%x1f%s%x1e",
    "-n", String(limit || 200), "--skip", String(skip || 0),
  ]);
  if (!ok(r)) return fail(r, "git log failed");
  const head = git(cwd, ["rev-parse", "HEAD"]);
  const branch = branchOf(cwd);
  const dirty = (git(cwd, ["status", "--porcelain"]).stdout || "").trim().length > 0;
  const out: GraphResult = {
    commits: parseGraph(r.stdout),
    head_sha: ok(head) ? (head.stdout || "").trim() : "",
    branch,
    dirty,
  };
  const stats = branchLineStats(cwd, branch);
  if (stats.branch_base) {
    out.branch_base = stats.branch_base;
    out.branch_added = stats.branch_added;
    out.branch_deleted = stats.branch_deleted;
  }
  return out;
}

function gitCommitFiles(cwd: string, sha: string): NumstatFile[] | { error: string } {
  const r = git(cwd, ["show", "-M", "-C", "--numstat", "--format=", sha]);
  if (!ok(r)) return fail(r, "git show failed");
  return parseNumstat(r.stdout);
}

function gitCommitDiff(cwd: string, sha: string, path: string): { diff: string } | { error: string } {
  const rl = git(cwd, ["rev-list", "--parents", "-n", "1", sha]);
  const isMerge = (rl.stdout || "").trim().split(" ").filter(Boolean).length > 2;
  const r = isMerge
    ? git(cwd, ["diff", "-M", "-C", "--no-color", `${sha}^1`, sha, "--", path])
    : git(cwd, ["show", "-M", "-C", "--no-color", "--format=", sha, "--", path]);
  if (r.error) return { error: r.error };
  return { diff: r.stdout || "" };
}

function gitWorkdirFiles(cwd: string): WorkdirFile[] | { error: string } {
  const s = git(cwd, ["-c", "core.quotePath=false", "status", "--porcelain=v1", "-z"]);
  if (!ok(s)) return fail(s, "git status failed");
  const ns = git(cwd, ["-c", "core.quotePath=false", "diff", "--numstat", "HEAD"]);
  return mergeWorkdirFiles(parseStatusPorcelain(s.stdout), ok(ns) ? parseNumstatCounts(ns.stdout) : {});
}

function gitWorkdirDiff(cwd: string, path: string): { diff: string } | { error: string } {
  const r = git(cwd, ["diff", "-M", "-C", "--no-color", "HEAD", "--", path]);
  if (r.error) return { error: r.error };
  return { diff: r.stdout || "" };
}

function gitStage(cwd: string, paths: string[]): { ok: true } | { error: string } {
  if (!paths || !paths.length) return { error: "no paths" };
  const r = git(cwd, ["add", "--"].concat(paths));
  if (!ok(r)) return fail(r, "git add failed");
  return { ok: true };
}

function gitUnstage(cwd: string, paths: string[]): { ok: true } | { error: string } {
  if (!paths || !paths.length) return { error: "no paths" };
  const r = git(cwd, ["restore", "--staged", "--"].concat(paths));
  if (!ok(r)) return fail(r, "git restore failed");
  return { ok: true };
}

function gitRevert(cwd: string, path: string): { reverted: true } | { error: string } {
  const s = git(cwd, ["-c", "core.quotePath=false", "status", "--porcelain=v1", "-z", "--", path]);
  const entry = parseStatusPorcelain(s.stdout).filter((e) => e.path === path)[0];
  if (entry && entry.status === "untracked") return { error: "untracked file — delete it manually" };
  if (entry && entry.status === "conflicted") return { error: "conflicted file — resolve it manually" };
  const r = git(cwd, ["checkout", "HEAD", "--", path]);
  if (!ok(r)) return fail(r, "git checkout failed");
  return { reverted: true };
}

function gitCommit(cwd: string, message: string): { committed: true; stdout: string } | { error: string } {
  const msg = (message || "").trim();
  if (!msg) return { error: "commit message is empty" };
  const r = git(cwd, ["commit", "-m", msg]);
  if (!ok(r)) return fail(r, "git commit failed");
  return { committed: true, stdout: r.stdout || "" };
}

function gitPush(cwd: string): { pushed: true; stdout: string; stderr: string } | { error: string } {
  const r = git(cwd, ["push"]);
  if (!ok(r)) return fail(r, "git push failed");
  return { pushed: true, stdout: r.stdout || "", stderr: r.stderr || "" };
}

function listChildDirs(cwd: string): string[] {
  const plat = typeof host.platform === "function" ? host.platform() : (host.platform as unknown as string);
  const cmd: ExecOpts =
    String(plat || "").toLowerCase().indexOf("win") !== -1
      ? { bin: "cmd", args: ["/c", "dir", "/b", "/ad", cwd] }
      : { bin: "ls", args: ["-1", cwd] };
  const r = exec(cmd);
  if (!ok(r)) return [];
  return (r.stdout || "").split(/\r?\n/).filter((l) => l.length > 0);
}

interface Repo {
  path: string;
  name: string;
  branch: string | null;
}

function gitRepos(cwd: string): Repo[] {
  const top = git(cwd, ["rev-parse", "--show-toplevel"]);
  if (ok(top) && (top.stdout || "").trim()) {
    const root = (top.stdout || "").trim();
    return [{ path: root, name: baseName(root), branch: branchOf(root) }];
  }
  const sep = cwd.replace(/[/\\]$/, "");
  const out: Repo[] = [];
  const names = listChildDirs(cwd);
  for (let i = 0; i < names.length && out.length < 20; i++) {
    const p = `${sep}/${names[i]}`;
    if (host.fileExists(`${p}/.git`)) out.push({ path: p, name: names[i], branch: branchOf(p) });
  }
  return out;
}

interface ViewArgs {
  cwd?: string;
  limit?: number;
  skip?: number;
  sha?: string;
  path?: string;
  paths?: string[];
  message?: string;
}

function viewCall(method: string, args: ViewArgs): unknown {
  args = args || {};
  const cwd = args.cwd;
  if (!cwd) return { error: "no cwd" };
  try {
    switch (method) {
      case "gitGraph":
        return gitGraph(cwd, args.limit, args.skip);
      case "gitCommitFiles":
        return gitCommitFiles(cwd, args.sha as string);
      case "gitCommitDiff":
        return gitCommitDiff(cwd, args.sha as string, args.path as string);
      case "gitWorkdirFiles":
        return gitWorkdirFiles(cwd);
      case "gitWorkdirDiff":
        return gitWorkdirDiff(cwd, args.path as string);
      case "gitStage":
        return gitStage(cwd, args.paths || (args.path ? [args.path] : []));
      case "gitUnstage":
        return gitUnstage(cwd, args.paths || (args.path ? [args.path] : []));
      case "gitCommit":
        return gitCommit(cwd, args.message as string);
      case "gitPush":
        return gitPush(cwd);
      case "gitRevert":
        return gitRevert(cwd, args.path as string);
      case "gitRepos":
        return gitRepos(cwd);
      default:
        return { error: `unknown method: ${method}` };
    }
  } catch (e) {
    return { error: String(e) };
  }
}

const plugin: PluginModule = {
  statusBubble,
  renderGlance,
  viewCall: viewCall as PluginModule["viewCall"],
  // exported for tests (pure logic, no host.exec)
  __test_parsePorcelain: parsePorcelain,
  __test_parseLog: parseLog,
  __test_parseGraph: parseGraph,
  __test_parseRefs: parseRefs,
  __test_parseNumstat: parseNumstat,
  __test_splitRename: splitRename,
  __test_parseStatusPorcelain: parseStatusPorcelain,
  __test_parseNumstatCounts: parseNumstatCounts,
  __test_mergeWorkdirFiles: mergeWorkdirFiles,
  __test_classifyPorcelain: classifyPorcelain,
};

export default plugin;

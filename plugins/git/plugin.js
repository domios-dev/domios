"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/git/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
function exec(opts) {
  const raw = host.exec(JSON.stringify(opts));
  try {
    return JSON.parse(raw);
  } catch (e) {
    return { error: String(e) };
  }
}
function git(cwd, args) {
  return exec({ bin: "git", args: ["-C", cwd].concat(args) });
}
function parsePorcelain(out) {
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
function parseLog(out) {
  return (out || "").split("\n").map((l) => l.trim()).filter((l) => l.length > 0);
}
function branchOf(cwd) {
  const r = git(cwd, ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (r.error || r.code !== 0) return null;
  return (r.stdout || "").trim() || null;
}
function statusBubble(ctx) {
  const cwd = ctx && ctx.cwd;
  if (!cwd) return null;
  const branch = branchOf(cwd);
  if (!branch) return null;
  const d = parsePorcelain(git(cwd, ["status", "--porcelain"]).stdout);
  return {
    label: branch,
    tone: "default",
    icon: "git",
    tooltip: d.total ? `${d.total} changed \u2014 open Git` : "clean \u2014 open Git",
    // Click opens the plugin's own Git view (its sandboxed iframe UI).
    action: "openPanel:view:git"
  };
}
function renderGlance(ctx) {
  const cwd = ctx && ctx.cwd;
  if (!cwd) {
    return {
      title: "Git",
      nodes: [{ kind: "text", text: "No working directory for this tab.", style: { tone: "muted" } }]
    };
  }
  const branch = branchOf(cwd);
  if (!branch) {
    return { title: "Git", nodes: [{ kind: "text", text: "Not a git repository.", style: { tone: "muted" } }] };
  }
  const d = parsePorcelain(git(cwd, ["status", "--porcelain"]).stdout);
  const commits = parseLog(git(cwd, ["log", "--oneline", "-n", "5"]).stdout);
  const nodes = [
    { kind: "keyVal", key: "Branch", value: branch },
    {
      kind: "keyVal",
      key: "Working tree",
      value: d.total ? `${d.total} changed (${d.staged} staged)` : "clean",
      style: { tone: d.total ? "warn" : "ok" }
    }
  ];
  if (commits.length) {
    nodes.push({ kind: "divider" });
    nodes.push({ kind: "text", text: "Recent commits", style: { tone: "muted" } });
    commits.forEach((c) => nodes.push({ kind: "text", text: c }));
  }
  nodes.push({ kind: "divider" });
  nodes.push({ kind: "button", label: "Open Git", action: "openPanel:view:git" });
  return { title: `Git \xB7 ${branch}`, nodes };
}
var FS = "";
var RS = "";
function parseRefs(s) {
  if (!s) return [];
  return s.split(", ").map((raw) => {
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
  }).filter((r) => r !== null);
}
function parseGraph(out) {
  return (out || "").split(RS).map((rec) => rec.charAt(0) === "\n" ? rec.slice(1) : rec).filter((rec) => rec.indexOf(FS) !== -1).map((rec) => {
    const f = rec.split(FS);
    return {
      sha: f[1] || "",
      parents: (f[2] || "").split(" ").filter((p) => p.length > 0),
      author: f[3] || "",
      date: f[4] || "",
      refs: parseRefs(f[5] || ""),
      subject: f[6] || ""
    };
  }).filter((c) => c.sha.length > 0);
}
function splitRename(s) {
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
      const collapse = (p) => p.replace(/\/\//g, "/");
      return { path: collapse(pre + newMid + post), old_path: collapse(pre + oldMid + post) };
    }
  }
  const top = s.indexOf(" => ");
  if (top !== -1) return { path: s.slice(top + 4), old_path: s.slice(0, top) };
  return { path: s, old_path: null };
}
function parseNumstat(out) {
  return (out || "").split("\n").map((line) => {
    if (!line) return null;
    const parts = line.split("	");
    if (parts.length < 3) return null;
    const addS = parts[0];
    const delS = parts[1];
    const r = splitRename(parts.slice(2).join("	"));
    const binary = addS === "-" && delS === "-";
    return {
      path: r.path,
      old_path: r.old_path,
      added: addS === "-" ? null : parseInt(addS, 10) || 0,
      deleted: delS === "-" ? null : parseInt(delS, 10) || 0,
      status: r.old_path ? "renamed" : binary ? "binary" : "modified"
    };
  }).filter((f) => f !== null);
}
function classifyPorcelain(x, y) {
  if (x === "?" && y === "?") return "untracked";
  if (x === "U" || y === "U" || x === "D" && y === "D" || x === "A" && y === "A") return "conflicted";
  if (x === "R" || y === "R") return "renamed";
  if (x === "D" || y === "D") return "deleted";
  if (x !== " " && x !== "?") return "staged";
  return "modified";
}
function parseStatusPorcelain(out) {
  const tokens = (out || "").split("\0").filter((t) => t.length > 0);
  const entries = [];
  for (let i = 0; i < tokens.length; i++) {
    const tok = tokens[i];
    if (tok.length < 3) continue;
    const x = tok.charAt(0);
    const y = tok.charAt(1);
    let oldPath = null;
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
      staged: x !== " " && x !== "?"
    });
  }
  return entries;
}
function parseNumstatCounts(out) {
  const map = {};
  (out || "").split("\n").forEach((line) => {
    if (!line) return;
    const parts = line.split("	");
    if (parts.length < 3) return;
    const r = splitRename(parts.slice(2).join("	"));
    map[r.path] = [
      parts[0] === "-" ? null : parseInt(parts[0], 10) || 0,
      parts[1] === "-" ? null : parseInt(parts[1], 10) || 0
    ];
  });
  return map;
}
function mergeWorkdirFiles(entries, counts) {
  return entries.map((e) => {
    const c = e.status === "untracked" ? null : counts[e.path];
    return {
      path: e.path,
      old_path: e.old_path,
      status: e.status,
      staged: e.staged,
      added: c ? c[0] : null,
      deleted: c ? c[1] : null
    };
  });
}
function baseName(p) {
  const parts = String(p).replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || String(p);
}
function ok(r) {
  return !!r && !r.error && r.code === 0;
}
function fail(r, msg) {
  return { error: r && (r.stderr || r.error) || msg };
}
function branchLineStats(cwd, branch) {
  if (!branch) return {};
  let base = null;
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
    const p = l.split("	");
    if (p.length < 2) return;
    if (p[0] !== "-") added += parseInt(p[0], 10) || 0;
    if (p[1] !== "-") deleted += parseInt(p[1], 10) || 0;
  });
  return { branch_base: base, branch_added: added, branch_deleted: deleted };
}
function gitGraph(cwd, limit, skip) {
  const r = git(cwd, [
    "-c",
    "core.quotePath=false",
    "log",
    "--branches",
    "--tags",
    "--remotes",
    "HEAD",
    "--topo-order",
    "--decorate=full",
    "--pretty=format:%x1f%H%x1f%P%x1f%an%x1f%aI%x1f%D%x1f%s%x1e",
    "-n",
    String(limit || 200),
    "--skip",
    String(skip || 0)
  ]);
  if (!ok(r)) return fail(r, "git log failed");
  const head = git(cwd, ["rev-parse", "HEAD"]);
  const branch = branchOf(cwd);
  const dirty = (git(cwd, ["status", "--porcelain"]).stdout || "").trim().length > 0;
  const out = {
    commits: parseGraph(r.stdout),
    head_sha: ok(head) ? (head.stdout || "").trim() : "",
    branch,
    dirty
  };
  const stats = branchLineStats(cwd, branch);
  if (stats.branch_base) {
    out.branch_base = stats.branch_base;
    out.branch_added = stats.branch_added;
    out.branch_deleted = stats.branch_deleted;
  }
  return out;
}
function gitCommitFiles(cwd, sha) {
  const r = git(cwd, ["show", "-M", "-C", "--numstat", "--format=", sha]);
  if (!ok(r)) return fail(r, "git show failed");
  return parseNumstat(r.stdout);
}
function gitCommitDiff(cwd, sha, path) {
  const rl = git(cwd, ["rev-list", "--parents", "-n", "1", sha]);
  const isMerge = (rl.stdout || "").trim().split(" ").filter(Boolean).length > 2;
  const r = isMerge ? git(cwd, ["diff", "-M", "-C", "--no-color", `${sha}^1`, sha, "--", path]) : git(cwd, ["show", "-M", "-C", "--no-color", "--format=", sha, "--", path]);
  if (r.error) return { error: r.error };
  return { diff: r.stdout || "" };
}
function gitWorkdirFiles(cwd) {
  const s = git(cwd, ["-c", "core.quotePath=false", "status", "--porcelain=v1", "-z"]);
  if (!ok(s)) return fail(s, "git status failed");
  const ns = git(cwd, ["-c", "core.quotePath=false", "diff", "--numstat", "HEAD"]);
  return mergeWorkdirFiles(parseStatusPorcelain(s.stdout), ok(ns) ? parseNumstatCounts(ns.stdout) : {});
}
function gitWorkdirDiff(cwd, path) {
  const r = git(cwd, ["diff", "-M", "-C", "--no-color", "HEAD", "--", path]);
  if (r.error) return { error: r.error };
  return { diff: r.stdout || "" };
}
function gitStage(cwd, paths) {
  if (!paths || !paths.length) return { error: "no paths" };
  const r = git(cwd, ["add", "--"].concat(paths));
  if (!ok(r)) return fail(r, "git add failed");
  return { ok: true };
}
function gitUnstage(cwd, paths) {
  if (!paths || !paths.length) return { error: "no paths" };
  const r = git(cwd, ["restore", "--staged", "--"].concat(paths));
  if (!ok(r)) return fail(r, "git restore failed");
  return { ok: true };
}
function gitRevert(cwd, path) {
  const s = git(cwd, ["-c", "core.quotePath=false", "status", "--porcelain=v1", "-z", "--", path]);
  const entry = parseStatusPorcelain(s.stdout).filter((e) => e.path === path)[0];
  if (entry && entry.status === "untracked") return { error: "untracked file \u2014 delete it manually" };
  if (entry && entry.status === "conflicted") return { error: "conflicted file \u2014 resolve it manually" };
  const r = git(cwd, ["checkout", "HEAD", "--", path]);
  if (!ok(r)) return fail(r, "git checkout failed");
  return { reverted: true };
}
function gitCommit(cwd, message) {
  const msg = (message || "").trim();
  if (!msg) return { error: "commit message is empty" };
  const r = git(cwd, ["commit", "-m", msg]);
  if (!ok(r)) return fail(r, "git commit failed");
  return { committed: true, stdout: r.stdout || "" };
}
function gitPush(cwd) {
  const r = git(cwd, ["push"]);
  if (!ok(r)) return fail(r, "git push failed");
  return { pushed: true, stdout: r.stdout || "", stderr: r.stderr || "" };
}
function isWindowsPlatform() {
  const plat = typeof host.platform === "function" ? host.platform() : host.platform;
  return String(plat || "").toLowerCase().indexOf("win") !== -1;
}
function listChildDirs(cwd) {
  const cmd = isWindowsPlatform() ? { bin: "cmd", args: ["/c", "dir", "/b", "/ad", cwd] } : { bin: "find", args: [cwd, "-mindepth", "1", "-maxdepth", "1", "-type", "d", "-print"] };
  const r = exec(cmd);
  if (!ok(r)) return [];
  if (isWindowsPlatform()) return (r.stdout || "").split(/\r?\n/).filter((l) => l.length > 0);
  return (r.stdout || "").split(/\r?\n/).map((line) => normalizeRepoPath(line)).filter((line) => line.length > 0).map((line) => baseName(line));
}
var REPO_DISCOVERY_MAX_DEPTH = 3;
var REPO_DISCOVERY_MAX_VISITED = 512;
var REPO_DISCOVERY_MAX_EMITTED = 64;
var REPO_DISCOVERY_SKIP_NAMES = [".git", "node_modules", "target", "dist", "build", "out", ".next", ".cache", "vendor"];
function normalizeRepoPath(p) {
  const raw = String(p || "").replace(/\\/g, "/");
  if (!raw) return "";
  if (/^[A-Za-z]:\/$/.test(raw)) return raw;
  return raw.replace(/\/+$/, "");
}
function joinRepoPath(parent, child) {
  const base = normalizeRepoPath(parent);
  const leaf = String(child || "").replace(/^[/\\]+|[/\\]+$/g, "");
  return base ? `${base}/${leaf}` : leaf;
}
function isSafeChildName(name) {
  if (!name || name === "." || name === "..") return false;
  if (name.indexOf("/") !== -1 || name.indexOf("\\") !== -1) return false;
  if (/^[A-Za-z]:/.test(name)) return false;
  if (name.indexOf("..") !== -1) return false;
  return true;
}
function shouldSkipRepoChild(name) {
  const probe = isWindowsPlatform() ? name.toLowerCase() : name;
  for (let i = 0; i < REPO_DISCOVERY_SKIP_NAMES.length; i++) {
    const skip = isWindowsPlatform() ? REPO_DISCOVERY_SKIP_NAMES[i].toLowerCase() : REPO_DISCOVERY_SKIP_NAMES[i];
    if (probe === skip) return true;
  }
  return false;
}
function sortedChildDirs(cwd) {
  const out = listChildDirs(cwd).filter((name) => isSafeChildName(name) && !shouldSkipRepoChild(name)).slice();
  out.sort();
  return out;
}
function isGitRepoDir(cwd) {
  return host.fileExists(`${cwd}/.git`);
}
function discoverGitRepos(cwd) {
  const start = normalizeRepoPath(cwd);
  if (!start) return [];
  const top = git(start, ["rev-parse", "--show-toplevel"]);
  const scanRoot = ok(top) && (top.stdout || "").trim() ? normalizeRepoPath((top.stdout || "").trim()) : start;
  const queue = [{ path: scanRoot, depth: 0 }];
  const visited = /* @__PURE__ */ new Set();
  const emitted = /* @__PURE__ */ new Set();
  const repos = [];
  for (let i = 0; i < queue.length; i++) {
    if (visited.size >= REPO_DISCOVERY_MAX_VISITED || repos.length >= REPO_DISCOVERY_MAX_EMITTED) break;
    const item = queue[i];
    const dir = normalizeRepoPath(item.path);
    if (!dir || visited.has(dir)) continue;
    visited.add(dir);
    if (isGitRepoDir(dir) || dir === scanRoot && ok(top)) {
      const repoPath = dir === scanRoot && ok(top) && (top.stdout || "").trim() ? normalizeRepoPath((top.stdout || "").trim()) : dir;
      if (!emitted.has(repoPath)) {
        emitted.add(repoPath);
        repos.push({ path: repoPath, name: baseName(repoPath), branch: branchOf(repoPath) });
        if (repos.length >= REPO_DISCOVERY_MAX_EMITTED) break;
      }
    }
    if (item.depth >= REPO_DISCOVERY_MAX_DEPTH || visited.size >= REPO_DISCOVERY_MAX_VISITED) continue;
    const children = sortedChildDirs(dir);
    for (let j = 0; j < children.length; j++) {
      if (visited.size + queue.length - i - 1 >= REPO_DISCOVERY_MAX_VISITED) break;
      queue.push({ path: joinRepoPath(dir, children[j]), depth: item.depth + 1 });
    }
  }
  repos.sort((a, b) => a.path.localeCompare(b.path));
  return repos;
}
function gitRepos(cwd) {
  return discoverGitRepos(cwd);
}
function viewCall(method, args) {
  args = args || {};
  const cwd = args.cwd;
  if (!cwd) return { error: "no cwd" };
  try {
    switch (method) {
      case "gitGraph":
        return gitGraph(cwd, args.limit, args.skip);
      case "gitCommitFiles":
        return gitCommitFiles(cwd, args.sha);
      case "gitCommitDiff":
        return gitCommitDiff(cwd, args.sha, args.path);
      case "gitWorkdirFiles":
        return gitWorkdirFiles(cwd);
      case "gitWorkdirDiff":
        return gitWorkdirDiff(cwd, args.path);
      case "gitStage":
        return gitStage(cwd, args.paths || (args.path ? [args.path] : []));
      case "gitUnstage":
        return gitUnstage(cwd, args.paths || (args.path ? [args.path] : []));
      case "gitCommit":
        return gitCommit(cwd, args.message);
      case "gitPush":
        return gitPush(cwd);
      case "gitRevert":
        return gitRevert(cwd, args.path);
      case "gitRepos":
        return gitRepos(cwd);
      default:
        return { error: `unknown method: ${method}` };
    }
  } catch (e) {
    return { error: String(e) };
  }
}
var plugin = {
  statusBubble,
  renderGlance,
  viewCall,
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
  __test_discoverGitRepos: discoverGitRepos,
  __test_gitRepoConstants: {
    maxDepth: REPO_DISCOVERY_MAX_DEPTH,
    maxVisited: REPO_DISCOVERY_MAX_VISITED,
    maxEmitted: REPO_DISCOVERY_MAX_EMITTED
  },
  __test_isSafeChildName: isSafeChildName
};
var plugin_default = plugin;

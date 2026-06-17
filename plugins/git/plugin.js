// Git plugin — CodeTerm's local-git presence, extracted into a plugin.
// Contributes a pane-footer status bubble (branch + dirty) and a glance popover
// (branch, changes, recent commits) with an "Open Git" button that opens the
// native Git panel. Runs `git -C <cwd>` via host.exec; the rich graph/diff panel
// stays native infra the glance links to.

function exec(opts) {
  var raw = host.exec(JSON.stringify(opts));
  try {
    return JSON.parse(raw);
  } catch (e) {
    return { error: String(e) };
  }
}

function git(cwd, args) {
  return exec({ bin: "git", args: ["-C", cwd].concat(args) });
}

// --- pure parsers (exported for tests) ---

// `git status --porcelain` → { total, staged, unstaged, untracked }.
function parsePorcelain(out) {
  var lines = (out || "").split("\n").filter(function (l) { return l.length > 0; });
  var staged = 0, unstaged = 0, untracked = 0;
  lines.forEach(function (l) {
    var x = l.charAt(0), y = l.charAt(1);
    if (x === "?" && y === "?") { untracked++; return; }
    if (x !== " " && x !== "?") staged++;
    if (y !== " " && y !== "?") unstaged++;
  });
  return { total: lines.length, staged: staged, unstaged: unstaged, untracked: untracked };
}

// `git log --oneline` → array of short lines (already one per commit).
function parseLog(out) {
  return (out || "")
    .split("\n")
    .map(function (l) { return l.trim(); })
    .filter(function (l) { return l.length > 0; });
}

function branchOf(cwd) {
  var r = git(cwd, ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (r.error || r.code !== 0) return null;
  return (r.stdout || "").trim() || null;
}

// --- capabilities ---

function statusBubble(ctx) {
  var cwd = ctx && ctx.cwd;
  if (!cwd) return null;
  var branch = branchOf(cwd);
  if (!branch) return null; // not a repo → no pill
  var d = parsePorcelain(git(cwd, ["status", "--porcelain"]).stdout);
  return {
    label: branch + (d.total ? " •" : ""),
    tone: d.total ? "warn" : "default",
    icon: "git",
    tooltip: d.total ? d.total + " changed — open Git" : "clean — open Git",
    // Click opens the plugin's own Git view (its sandboxed iframe UI).
    action: "openPanel:view:git",
  };
}

function renderGlance(ctx) {
  var cwd = ctx && ctx.cwd;
  if (!cwd) {
    return { title: "Git", nodes: [{ kind: "text", text: "No working directory for this pane.", style: { tone: "muted" } }] };
  }
  var branch = branchOf(cwd);
  if (!branch) {
    return { title: "Git", nodes: [{ kind: "text", text: "Not a git repository.", style: { tone: "muted" } }] };
  }
  var d = parsePorcelain(git(cwd, ["status", "--porcelain"]).stdout);
  var commits = parseLog(git(cwd, ["log", "--oneline", "-n", "5"]).stdout);

  var nodes = [
    { kind: "keyVal", key: "Branch", value: branch },
    {
      kind: "keyVal",
      key: "Working tree",
      value: d.total ? d.total + " changed (" + d.staged + " staged)" : "clean",
      style: { tone: d.total ? "warn" : "ok" },
    },
  ];
  if (commits.length) {
    nodes.push({ kind: "divider" });
    nodes.push({ kind: "text", text: "Recent commits", style: { tone: "muted" } });
    commits.forEach(function (c) { nodes.push({ kind: "text", text: c }); });
  }
  nodes.push({ kind: "divider" });
  nodes.push({ kind: "button", label: "Open Git", action: "openPanel:view:git" });
  return { title: "Git · " + branch, nodes: nodes };
}

// --- view ops: faithful port of the former native git_routes.rs ---
// The plugin's iframe UI (ui/) calls these via window.ct.invoke(method, args).
// Every op shells `git -C <cwd>` and reproduces the exact REST output shapes.

var FS = "\x1f"; // field separator inside the graph pretty-format
var RS = "\x1e"; // record separator between commits

// `%D`-style ref decoration → [{ name, kind }].
function parseRefs(s) {
  if (!s) return [];
  return s.split(", ").map(function (raw) {
    raw = raw.trim();
    if (!raw) return null;
    if (raw.indexOf("tag: ") === 0) {
      var t = raw.slice(5);
      if (t.indexOf("refs/tags/") === 0) t = t.slice(10);
      return { name: t.replace(/\^\{\}$/, ""), kind: "tag" };
    }
    if (raw.indexOf("HEAD -> ") === 0) {
      var h = raw.slice(8);
      if (h.indexOf("refs/heads/") === 0) h = h.slice(11);
      return { name: h, kind: "head" };
    }
    if (raw === "HEAD") return { name: "HEAD", kind: "head" };
    if (raw.indexOf("refs/heads/") === 0) return { name: raw.slice(11), kind: "branch" };
    if (raw.indexOf("refs/remotes/") === 0) return { name: raw.slice(13), kind: "remote" };
    if (raw.indexOf("refs/tags/") === 0) return { name: raw.slice(10).replace(/\^\{\}$/, ""), kind: "tag" };
    return { name: raw, kind: "branch" };
  }).filter(Boolean);
}

function parseGraph(out) {
  return (out || "")
    .split(RS)
    .map(function (rec) { return rec.charAt(0) === "\n" ? rec.slice(1) : rec; })
    .filter(function (rec) { return rec.indexOf(FS) !== -1; })
    .map(function (rec) {
      var f = rec.split(FS); // [0]="" [1]sha [2]parents [3]author [4]date [5]refs [6]subject
      return {
        sha: f[1] || "",
        parents: (f[2] || "").split(" ").filter(function (p) { return p.length > 0; }),
        author: f[3] || "",
        date: f[4] || "",
        refs: parseRefs(f[5] || ""),
        subject: f[6] || "",
      };
    })
    .filter(function (c) { return c.sha.length > 0; });
}

// `old => new` and `dir/{old => new}/file` rename forms → { path, old_path }.
function splitRename(s) {
  var brace = s.indexOf("{");
  if (brace !== -1) {
    var close = s.indexOf("}", brace);
    var arrow = brace !== -1 && close !== -1 ? s.slice(brace + 1, close).indexOf(" => ") : -1;
    if (close !== -1 && arrow !== -1) {
      var pre = s.slice(0, brace);
      var inner = s.slice(brace + 1, close);
      var post = s.slice(close + 1);
      var a = inner.indexOf(" => ");
      var oldMid = inner.slice(0, a);
      var newMid = inner.slice(a + 4);
      var collapse = function (p) { return p.replace(/\/\//g, "/"); };
      return { path: collapse(pre + newMid + post), old_path: collapse(pre + oldMid + post) };
    }
  }
  var top = s.indexOf(" => ");
  if (top !== -1) return { path: s.slice(top + 4), old_path: s.slice(0, top) };
  return { path: s, old_path: null };
}

// `git show --numstat` body → [{ path, old_path, added, deleted, status }].
function parseNumstat(out) {
  return (out || "").split("\n").map(function (line) {
    if (!line) return null;
    var parts = line.split("\t");
    if (parts.length < 3) return null;
    var addS = parts[0], delS = parts[1];
    var r = splitRename(parts.slice(2).join("\t"));
    var binary = addS === "-" && delS === "-";
    return {
      path: r.path,
      old_path: r.old_path,
      added: addS === "-" ? null : (parseInt(addS, 10) || 0),
      deleted: delS === "-" ? null : (parseInt(delS, 10) || 0),
      status: r.old_path ? "renamed" : binary ? "binary" : "modified",
    };
  }).filter(Boolean);
}

function classifyPorcelain(x, y) {
  if (x === "?" && y === "?") return "untracked";
  if (x === "U" || y === "U" || (x === "D" && y === "D") || (x === "A" && y === "A")) return "conflicted";
  if (x === "R" || y === "R") return "renamed";
  if (x === "D" || y === "D") return "deleted";
  if (x !== " " && x !== "?") return "staged";
  return "modified";
}

// `git status --porcelain=v1 -z` → [{ path, old_path, status, staged }].
function parseStatusPorcelain(out) {
  var tokens = (out || "").split("\0").filter(function (t) { return t.length > 0; });
  var entries = [];
  for (var i = 0; i < tokens.length; i++) {
    var tok = tokens[i];
    if (tok.length < 3) continue;
    var x = tok.charAt(0), y = tok.charAt(1);
    var oldPath = null;
    if (x === "R" || x === "C" || y === "R" || y === "C") {
      if (i + 1 < tokens.length) { oldPath = tokens[i + 1]; i++; }
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

// `git diff --numstat HEAD` → { path: [added, deleted] }.
function parseNumstatCounts(out) {
  var map = {};
  (out || "").split("\n").forEach(function (line) {
    if (!line) return;
    var parts = line.split("\t");
    if (parts.length < 3) return;
    var r = splitRename(parts.slice(2).join("\t"));
    map[r.path] = [parts[0] === "-" ? null : (parseInt(parts[0], 10) || 0), parts[1] === "-" ? null : (parseInt(parts[1], 10) || 0)];
  });
  return map;
}

function mergeWorkdirFiles(entries, counts) {
  return entries.map(function (e) {
    var c = e.status === "untracked" ? null : counts[e.path];
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

function baseName(p) {
  var parts = String(p).replace(/[\/\\]+$/, "").split(/[\/\\]/);
  return parts[parts.length - 1] || String(p);
}

function ok(r) { return r && !r.error && r.code === 0; }
function fail(r, msg) { return { error: (r && (r.stderr || r.error)) || msg }; }

// --- ops ---

function branchLineStats(cwd, branch) {
  if (!branch) return {};
  var base = null;
  var up = git(cwd, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
  if (ok(up)) base = (up.stdout || "").trim() || null;
  if (!base) {
    ["origin/main", "origin/master", "main", "master"].some(function (cand) {
      if (ok(git(cwd, ["rev-parse", "--verify", "-q", cand]))) { base = cand; return true; }
      return false;
    });
  }
  if (!base) return {};
  var mb = git(cwd, ["merge-base", base, "HEAD"]);
  if (!ok(mb)) return {};
  var baseSha = (mb.stdout || "").trim();
  if (!baseSha) return {};
  var ns = git(cwd, ["diff", "--numstat", baseSha, "HEAD"]);
  if (!ok(ns)) return {};
  var added = 0, deleted = 0;
  (ns.stdout || "").split("\n").forEach(function (l) {
    if (!l) return;
    var p = l.split("\t");
    if (p.length < 2) return;
    if (p[0] !== "-") added += parseInt(p[0], 10) || 0;
    if (p[1] !== "-") deleted += parseInt(p[1], 10) || 0;
  });
  return { branch_base: base, branch_added: added, branch_deleted: deleted };
}

function gitGraph(cwd, limit, skip) {
  var r = git(cwd, [
    "-c", "core.quotePath=false", "log", "--branches", "--tags", "--remotes", "HEAD",
    "--topo-order", "--decorate=full",
    "--pretty=format:%x1f%H%x1f%P%x1f%an%x1f%aI%x1f%D%x1f%s%x1e",
    "-n", String(limit || 200), "--skip", String(skip || 0),
  ]);
  if (!ok(r)) return fail(r, "git log failed");
  var head = git(cwd, ["rev-parse", "HEAD"]);
  var branch = branchOf(cwd);
  var dirty = (git(cwd, ["status", "--porcelain"]).stdout || "").trim().length > 0;
  var out = {
    commits: parseGraph(r.stdout),
    head_sha: ok(head) ? (head.stdout || "").trim() : "",
    branch: branch,
    dirty: dirty,
  };
  var stats = branchLineStats(cwd, branch);
  if (stats.branch_base) {
    out.branch_base = stats.branch_base;
    out.branch_added = stats.branch_added;
    out.branch_deleted = stats.branch_deleted;
  }
  return out;
}

function gitCommitFiles(cwd, sha) {
  var r = git(cwd, ["show", "-M", "-C", "--numstat", "--format=", sha]);
  if (!ok(r)) return fail(r, "git show failed");
  return parseNumstat(r.stdout);
}

function gitCommitDiff(cwd, sha, path) {
  var rl = git(cwd, ["rev-list", "--parents", "-n", "1", sha]);
  var isMerge = (rl.stdout || "").trim().split(" ").filter(Boolean).length > 2;
  var r = isMerge
    ? git(cwd, ["diff", "-M", "-C", "--no-color", sha + "^1", sha, "--", path])
    : git(cwd, ["show", "-M", "-C", "--no-color", "--format=", sha, "--", path]);
  if (r.error) return { error: r.error };
  return { diff: r.stdout || "" };
}

function gitWorkdirFiles(cwd) {
  var s = git(cwd, ["-c", "core.quotePath=false", "status", "--porcelain=v1", "-z"]);
  if (!ok(s)) return fail(s, "git status failed");
  var ns = git(cwd, ["-c", "core.quotePath=false", "diff", "--numstat", "HEAD"]);
  return mergeWorkdirFiles(parseStatusPorcelain(s.stdout), ok(ns) ? parseNumstatCounts(ns.stdout) : {});
}

function gitWorkdirDiff(cwd, path) {
  var r = git(cwd, ["diff", "-M", "-C", "--no-color", "HEAD", "--", path]);
  if (r.error) return { error: r.error };
  return { diff: r.stdout || "" };
}

function gitStage(cwd, paths) {
  if (!paths || !paths.length) return { error: "no paths" };
  var r = git(cwd, ["add", "--"].concat(paths));
  if (!ok(r)) return fail(r, "git add failed");
  return { ok: true };
}

function gitUnstage(cwd, paths) {
  if (!paths || !paths.length) return { error: "no paths" };
  var r = git(cwd, ["restore", "--staged", "--"].concat(paths));
  if (!ok(r)) return fail(r, "git restore failed");
  return { ok: true };
}

function gitRevert(cwd, path) {
  var s = git(cwd, ["-c", "core.quotePath=false", "status", "--porcelain=v1", "-z", "--", path]);
  var entry = parseStatusPorcelain(s.stdout).filter(function (e) { return e.path === path; })[0];
  if (entry && entry.status === "untracked") return { error: "untracked file — delete it manually" };
  if (entry && entry.status === "conflicted") return { error: "conflicted file — resolve it manually" };
  var r = git(cwd, ["checkout", "HEAD", "--", path]);
  if (!ok(r)) return fail(r, "git checkout failed");
  return { reverted: true };
}

function gitCommit(cwd, message) {
  var msg = (message || "").trim();
  if (!msg) return { error: "commit message is empty" };
  var r = git(cwd, ["commit", "-m", msg]);
  if (!ok(r)) return fail(r, "git commit failed");
  return { committed: true, stdout: r.stdout || "" };
}

function gitPush(cwd) {
  var r = git(cwd, ["push"]);
  if (!ok(r)) return fail(r, "git push failed");
  return { pushed: true, stdout: r.stdout || "", stderr: r.stderr || "" };
}

function listChildDirs(cwd) {
  var plat = typeof host.platform === "function" ? host.platform() : host.platform;
  var cmd = String(plat || "").toLowerCase().indexOf("win") !== -1
    ? { bin: "cmd", args: ["/c", "dir", "/b", "/ad", cwd] }
    : { bin: "ls", args: ["-1", cwd] };
  var r = exec(cmd);
  if (!ok(r)) return [];
  return (r.stdout || "").split(/\r?\n/).filter(function (l) { return l.length > 0; });
}

function gitRepos(cwd) {
  var top = git(cwd, ["rev-parse", "--show-toplevel"]);
  if (ok(top) && (top.stdout || "").trim()) {
    var root = (top.stdout || "").trim();
    return [{ path: root, name: baseName(root), branch: branchOf(root) }];
  }
  var sep = cwd.replace(/[\/\\]$/, "");
  var out = [];
  var names = listChildDirs(cwd);
  for (var i = 0; i < names.length && out.length < 20; i++) {
    var p = sep + "/" + names[i];
    if (host.fileExists(p + "/.git")) out.push({ path: p, name: names[i], branch: branchOf(p) });
  }
  return out;
}

function viewCall(method, args) {
  args = args || {};
  var cwd = args.cwd;
  if (!cwd) return { error: "no cwd" };
  try {
    switch (method) {
      case "gitGraph": return gitGraph(cwd, args.limit, args.skip);
      case "gitCommitFiles": return gitCommitFiles(cwd, args.sha);
      case "gitCommitDiff": return gitCommitDiff(cwd, args.sha, args.path);
      case "gitWorkdirFiles": return gitWorkdirFiles(cwd);
      case "gitWorkdirDiff": return gitWorkdirDiff(cwd, args.path);
      case "gitStage": return gitStage(cwd, args.paths || (args.path ? [args.path] : []));
      case "gitUnstage": return gitUnstage(cwd, args.paths || (args.path ? [args.path] : []));
      case "gitCommit": return gitCommit(cwd, args.message);
      case "gitPush": return gitPush(cwd);
      case "gitRevert": return gitRevert(cwd, args.path);
      case "gitRepos": return gitRepos(cwd);
      default: return { error: "unknown method: " + method };
    }
  } catch (e) {
    return { error: String(e) };
  }
}

module.exports.default = {
  statusBubble: statusBubble,
  renderGlance: renderGlance,
  viewCall: viewCall,
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

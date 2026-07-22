/**
 * Git plugin discovery tests.
 * Run: npx tsx plugins/git/tests/plugin.test.ts
 */

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`);
  }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

type ExecMap = Record<string, { code: number; stdout?: string; stderr?: string }>;

function repoTopLevels(path: string): string[] {
  const out: string[] = [];
  let cur = path.replace(/\\/g, "/").replace(/\/+$/, "");
  while (cur) {
    out.push(cur);
    const idx = cur.lastIndexOf("/");
    if (idx <= 0) break;
    cur = cur.slice(0, idx);
  }
  return out;
}

function makeHost(opts: {
  childDirs: Record<string, string[]>;
  gitDirs: string[];
  branchByRepo?: Record<string, string>;
  explicitExec?: ExecMap;
  platform?: "linux" | "windows";
}) {
  const platform = opts.platform || "linux";
  const gitDirSet = new Set(opts.gitDirs.map((p) => `${p.replace(/\\/g, "/").replace(/\/+$/, "")}/.git`));
  const branchByRepo = opts.branchByRepo || {};
  const childDirs = Object.fromEntries(
    Object.entries(opts.childDirs).map(([k, v]) => [k.replace(/\\/g, "/").replace(/\/+$/, ""), v.slice()]),
  );
  const explicitExec = opts.explicitExec || {};
  const execCalls: string[] = [];

  return {
    execCalls,
    host: {
      platform: () => platform,
      fileExists: (p: string) => gitDirSet.has(p.replace(/\\/g, "/").replace(/\/+$/, "")),
      exec: (raw: string) => {
        const parsed = JSON.parse(raw) as { bin: string; args: string[] };
        const key = `${parsed.bin} ${parsed.args.join(" ")}`;
        execCalls.push(key);
        if (explicitExec[key]) return JSON.stringify(explicitExec[key]);

        if (
          parsed.bin === "find" &&
          JSON.stringify(parsed.args.slice(1)) === JSON.stringify(["-mindepth", "1", "-maxdepth", "1", "-type", "d", "-print"])
        ) {
          const dir = parsed.args[0].replace(/\\/g, "/").replace(/\/+$/, "");
          return JSON.stringify({
            code: 0,
            stdout: (childDirs[dir] || []).map((child) => `${dir}/${child}`.replace(/\/+/g, "/")).join("\n"),
          });
        }
        if (parsed.bin === "cmd" && parsed.args[0] === "/c" && parsed.args[1] === "dir") {
          const dir = parsed.args[4].replace(/\\/g, "/").replace(/\/+$/, "");
          return JSON.stringify({ code: 0, stdout: (childDirs[dir] || []).join("\n") });
        }

        if (parsed.bin === "git" && parsed.args[0] === "-C") {
          const cwd = parsed.args[1].replace(/\\/g, "/").replace(/\/+$/, "");
          const cmd = parsed.args.slice(2).join(" ");
          if (cmd === "rev-parse --show-toplevel") {
            const top = repoTopLevels(cwd).find((candidate) => gitDirSet.has(`${candidate}/.git`));
            return JSON.stringify(top ? { code: 0, stdout: `${top}\n` } : { code: 128, stderr: "not a repo" });
          }
          if (cmd === "rev-parse --abbrev-ref HEAD") {
            return JSON.stringify(
              branchByRepo[cwd]
                ? { code: 0, stdout: `${branchByRepo[cwd]}\n` }
                : { code: 128, stderr: "not a repo" },
            );
          }
        }

        return JSON.stringify({ code: 1, stderr: `unhandled exec: ${key}` });
      },
    },
  };
}

async function loadPlugin(host: unknown) {
  (globalThis as { host?: unknown }).host = host;
  const mod = await import(`../src/plugin.ts?case=${passed + failed}`);
  return mod.default as {
    __test_discoverGitRepos: (cwd: string) => Array<{ path: string; name: string; branch: string | null }>;
    __test_gitRepoConstants: { maxDepth: number; maxVisited: number; maxEmitted: number };
    __test_isSafeChildName: (name: string) => boolean;
  };
}

console.log("\n--- gitRepos discovery ---\n");

{
  const fixture = makeHost({
    gitDirs: ["/ws/root", "/ws/root/child-a", "/ws/root/child-b", "/ws/root/child-c", "/ws/root/worktrees/linked"],
    branchByRepo: {
      "/ws/root": "main",
      "/ws/root/child-a": "feature-a",
      "/ws/root/child-b": "feature-b",
      "/ws/root/child-c": "feature-c",
      "/ws/root/worktrees/linked": "wt-linked",
    },
    childDirs: {
      "/ws/root": ["child-b", "worktrees", "child-a", "docs", "child-c"],
      "/ws/root/child-a": [],
      "/ws/root/child-b": [],
      "/ws/root/child-c": [],
      "/ws/root/docs": [],
      "/ws/root/worktrees": ["linked"],
      "/ws/root/worktrees/linked": [],
    },
  });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/ws/root/src");
  eq(
    repos.map((repo) => repo.path),
    ["/ws/root", "/ws/root/child-a", "/ws/root/child-b", "/ws/root/child-c", "/ws/root/worktrees/linked"],
    "root-first sorted discovery keeps nested children and .git-file worktree support",
  );
}

{
  const fixture = makeHost({
    gitDirs: ["/scan/plain/nested"],
    branchByRepo: { "/scan/plain/nested": "topic" },
    childDirs: {
      "/scan/plain": ["README.md", "nested", "notes.txt"],
      "/scan/plain/nested": [],
    },
  });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/scan/plain");
  eq(repos.map((repo) => repo.path), ["/scan/plain/nested"], "non-repo parents discover nested repos and never queue plain files");
  assert(
    fixture.execCalls.includes("find /scan/plain -mindepth 1 -maxdepth 1 -type d -print"),
    "Unix discovery uses portable find -print argv",
  );
}

{
  const childDirs: Record<string, string[]> = {
    "/skip/root": ["good", "node_modules", "target", ".cache", "vendor", "dist", ".next", "build", "out"],
    "/skip/root/good": [],
  };
  const gitDirs = ["/skip/root", "/skip/root/good"];
  const skipParents = ["/skip/root/node_modules", "/skip/root/target", "/skip/root/.cache", "/skip/root/vendor", "/skip/root/dist", "/skip/root/.next", "/skip/root/build", "/skip/root/out"];
  for (let i = 0; i < skipParents.length; i++) {
    childDirs[skipParents[i]] = [];
    for (let j = 0; j < 120; j++) childDirs[skipParents[i]].push(`fake-${i}-${j}`);
    for (let j = 0; j < 120; j++) gitDirs.push(`${skipParents[i]}/fake-${i}-${j}`);
  }
  const fixture = makeHost({ gitDirs, childDirs, branchByRepo: { "/skip/root": "main", "/skip/root/good": "good" } });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/skip/root");
  eq(
    repos.map((repo) => repo.path),
    ["/skip/root", "/skip/root/good"],
    "skip-set trees are neither visited nor emitted even when they contain many fake repos",
  );
}

{
  const fixture = makeHost({
    gitDirs: ["/depth/root", "/depth/root/dup", "/depth/root/level1/level2/level3"],
    childDirs: {
      "/depth/root": ["dup", "dup", "level1"],
      "/depth/root/dup": [],
      "/depth/root/level1": ["level2"],
      "/depth/root/level1/level2": ["level3"],
      "/depth/root/level1/level2/level3": ["level4"],
      "/depth/root/level1/level2/level3/level4": ["too-deep"],
    },
  });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/depth/root");
  eq(
    repos.map((repo) => repo.path),
    ["/depth/root", "/depth/root/dup", "/depth/root/level1/level2/level3"],
    "duplicate entries dedupe deterministically and depth 3 is the traversal ceiling",
  );
}

{
  const gitDirs = ["/bounds/root"];
  for (let i = 0; i < 600; i++) gitDirs.push(`/bounds/root/repo-${String(i).padStart(3, "0")}`);
  const fixture = makeHost({
    gitDirs,
    childDirs: Object.fromEntries([
      ["/bounds/root", Array.from({ length: 600 }, (_, i) => `repo-${String(i).padStart(3, "0")}`)],
      ...Array.from({ length: 600 }, (_, i) => [`/bounds/root/repo-${String(i).padStart(3, "0")}`, []] as const),
    ]),
  });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/bounds/root");
  eq(repos.length, plugin.__test_gitRepoConstants.maxEmitted, "3/512/64 work budget still caps emitted repositories at 64");
}

{
  const fixture = makeHost({
    gitDirs: ["/unsafe/root", "/unsafe/root/safe-child"],
    childDirs: {
      "/unsafe/root": ["safe-child", ".", "..", "/abs", "nested/child", "nested\\child", "a..b", "C:drive"],
      "/unsafe/root/safe-child": [],
    },
  });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/unsafe/root");
  const lexical = [
    plugin.__test_isSafeChildName("."),
    plugin.__test_isSafeChildName(".."),
    plugin.__test_isSafeChildName("/abs"),
    plugin.__test_isSafeChildName("nested/child"),
    plugin.__test_isSafeChildName("nested\\child"),
    plugin.__test_isSafeChildName("a..b"),
    plugin.__test_isSafeChildName("C:drive"),
    plugin.__test_isSafeChildName("safe-child"),
  ];
  eq(
    { repos: repos.map((repo) => repo.path), lexical },
    { repos: ["/unsafe/root", "/unsafe/root/safe-child"], lexical: [false, false, false, false, false, false, false, true] },
    "lexical containment rejects dot-segments, separators, and absolute-looking names before queueing",
  );
}

{
  const fixture = makeHost({
    gitDirs: ["/case/root", "/case/root/kept"],
    platform: "windows",
    childDirs: {
      "/case/root": ["Node_Modules", "TARGET", "kept"],
      "/case/root/kept": [],
      "/case/root/Node_Modules": ["ignored"],
      "/case/root/TARGET": ["ignored-2"],
    },
  });
  const plugin = await loadPlugin(fixture.host);
  const repos = plugin.__test_discoverGitRepos("/case/root");
  eq(repos.map((repo) => repo.path), ["/case/root", "/case/root/kept"], "Windows skip-set matching is case-insensitive");
}

console.log("\nResidual review note: lexical containment is enforced in QuickJS, but junction/symlink identity cannot be proven here without a safe host stat/realpath surface.\n");
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);

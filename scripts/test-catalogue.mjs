#!/usr/bin/env node
// Caller: build.sh script-guards; CI tests.yml; scripts ci-rust-archives.mjs, plugin-tests.sh, tests/test-catalogue.test.mjs; skill testing
// Test catalogue: tests/catalogue.json runners whose membership is discovery output, and the stale/flaky overlay.
//
//   validate                  discover every runner; fail on an unowned, doubly owned or empty test-looking file, an invalid
//                             field, an unresolved overlay key, a vitest-owned list disagreement or a stale tests/README.md
//   select [--tier pr|release|manual] [--kind K] [--format text|json|github-matrix]
//   list --entry ID           the runner's discovered files, one per line
//   run --entry ID            the runner's exec over its discovered files; stale and platform-red markers for this platform are skipped
//   stale                     the overlay markers by class: stale, flaky, platform-red
//   readme [--check]          regenerate (or check) tests/README.md
//   --root DIR                operate on another tree (fixture tests)
// Rust membership comes from `cargo nextest list --message-format json` files named by CODETERM_CATALOGUE_NEXTEST_JSON
// (path-delimiter separated) for every package they list, and from the static scan in rust-test-inventory.mjs otherwise.
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseCargoToml, readInventoryFile, scanCrate, testKey, workspaceCrates } from "./rust-test-inventory.mjs";

export const PATHS = {
  catalogue: "tests/catalogue.json",
  overlay: "tests/catalogue.overlay.json",
  schema: "tests/catalogue.schema.json",
  readme: "tests/README.md",
  vitest: "scripts/vitest-owned-tests.json",
};
const TS_UNIT_ID = "ts-unit";
const FILE_METHODS = new Set(["glob", "pytest-collect", "playwright-list"]);
const RUST_METHODS = new Set(["nextest-list", "static-scan"]);
export const MARKER_CLASSES = [
  ["stale", "STALE (no longer matches the product; left out, never healthy)"],
  ["flaky", "FLAKY (runs, proves nothing; flagged, never healthy)"],
  ["platform_red", "PLATFORM-RED (red on the listed platforms, skipped there; never healthy, not safe to delete)"],
];
const PLATFORM = { win32: "windows", darwin: "macos", linux: "linux" }[process.platform] ?? process.platform;
const posix = path.posix;
const toPosix = (p) => p.replace(/\\/g, "/");

const escapeRe = (s) => s.replace(/[.+^$(){}|[\]\\]/g, "\\$&");
function globSource(glob) {
  let re = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      const slash = glob[i + 2] === "/";
      re += slash ? "(?:[^/]*/)*" : ".*";
      i += slash ? 2 : 1;
    } else if (c === "*") re += "[^/]*";
    else if (c === "?") re += "[^/]";
    else if (c === "{" && glob.indexOf("}", i) > i) {
      const end = glob.indexOf("}", i);
      re += `(?:${glob.slice(i + 1, end).split(",").map(globSource).join("|")})`;
      i = end;
    } else re += escapeRe(c);
  }
  return re;
}
const globCache = new Map();
export function globToRegExp(glob) {
  if (!globCache.has(glob)) globCache.set(glob, new RegExp(`^${globSource(glob)}$`));
  return globCache.get(glob);
}
export const matchesAny = (file, globs = []) => globs.some((g) => globToRegExp(g).test(file));

// The subset of JSON Schema 2020-12 that tests/catalogue.schema.json uses; no npm validator in the gate.
export function schemaErrors(root, node, value, at = "$") {
  const resolve = (ref) => ref.replace(/^#\//, "").split("/").reduce((s, k) => s?.[k], root);
  const isType = (t, v) =>
    t === "integer" ? Number.isInteger(v)
    : t === "array" ? Array.isArray(v)
    : t === "object" ? v !== null && typeof v === "object" && !Array.isArray(v)
    : t === "null" ? v === null
    : typeof v === t;
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const passes = (s, v) => {
    const out = [];
    visit(s, v, at, out);
    return out.length === 0;
  };
  function visit(s, v, p, out) {
    if (s.$ref) return visit(resolve(s.$ref), v, p, out);
    const err = (m) => out.push(`${p}: ${m}`);
    if (s.type && ![].concat(s.type).some((t) => isType(t, v))) return err(`must be ${[].concat(s.type).join(" or ")}`);
    if ("const" in s && !same(s.const, v)) err(`must be ${JSON.stringify(s.const)}`);
    if (s.enum && !s.enum.some((e) => same(e, v))) err(`must be one of ${s.enum.join(", ")}, not ${JSON.stringify(v)}`);
    if (typeof v === "string") {
      if (s.minLength !== undefined && v.length < s.minLength) err(`is shorter than ${s.minLength}`);
      if (s.pattern && !new RegExp(s.pattern).test(v)) err(`${JSON.stringify(v)} does not match ${s.pattern}`);
    }
    if (typeof v === "number") {
      if (s.minimum !== undefined && v < s.minimum) err(`is below ${s.minimum}`);
      if (s.maximum !== undefined && v > s.maximum) err(`is above ${s.maximum}`);
    }
    if (Array.isArray(v)) {
      if (s.minItems !== undefined && v.length < s.minItems) err(`needs at least ${s.minItems} item(s)`);
      if (s.uniqueItems && new Set(v.map((x) => JSON.stringify(x))).size !== v.length) err("has duplicate items");
      if (s.items) v.forEach((x, i) => visit(s.items, x, `${p}[${i}]`, out));
    }
    if (isType("object", v)) {
      for (const k of s.required ?? []) if (!(k in v)) err(`missing required field ${k}`);
      if (s.minProperties !== undefined && Object.keys(v).length < s.minProperties) err(`needs at least ${s.minProperties} field(s)`);
      for (const [k, sub] of Object.entries(s.properties ?? {})) if (k in v) visit(sub, v[k], `${p}.${k}`, out);
      if (s.additionalProperties === false) for (const k of Object.keys(v)) if (!(k in (s.properties ?? {}))) err(`unknown field ${k}`);
    }
    for (const sub of s.allOf ?? []) visit(sub, v, p, out);
    if (s.anyOf && !s.anyOf.some((sub) => passes(sub, v))) err("matches no allowed form");
    if (s.oneOf) {
      const n = s.oneOf.filter((sub) => passes(sub, v)).length;
      if (n !== 1) err(n ? "matches more than one form" : "matches no allowed form");
    }
    if (s.if && passes(s.if, v) && s.then) visit(s.then, v, p, out);
  }
  const errors = [];
  visit(node, value, at, errors);
  return errors;
}

function walkFiles(root, rel = "", out = []) {
  for (const e of readdirSync(path.join(root, rel), { withFileTypes: true })) {
    if (e.name === ".git" || e.name === "node_modules") continue;
    const child = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) walkFiles(root, child, out);
    else if (e.isFile()) out.push(child);
  }
  return out;
}

// Tracked plus untracked-but-not-ignored files; a tree outside git (fixtures) is walked instead.
export function listRepoFiles(root) {
  const top = spawnSync("git", ["-C", root, "rev-parse", "--show-toplevel"], { encoding: "utf8" });
  const key = (p) => (process.platform === "win32" ? path.resolve(p).toLowerCase() : path.resolve(p));
  if (top.status !== 0 || key(top.stdout.trim()) !== key(root)) return walkFiles(root).sort();
  const ls = spawnSync("git", ["-C", root, "ls-files", "-z", "-co", "--exclude-standard"], { encoding: "utf8", maxBuffer: 1 << 28 });
  if (ls.status !== 0) throw new Error(`git ls-files failed: ${ls.stderr}`);
  return [...new Set(ls.stdout.split("\0").filter(Boolean))].filter((f) => existsSync(path.join(root, f))).sort();
}

export function rustPackages(root, runners, nextestFiles = []) {
  const abs = toPosix(path.resolve(root));
  const crates = new Map(workspaceCrates(abs).crates.map((c) => [c.name, c]));
  for (const r of runners) {
    if (!RUST_METHODS.has(r.discovery?.method) || crates.has(r.package) || !r.manifest) continue;
    const file = path.join(root, r.manifest);
    if (!existsSync(file)) continue;
    const manifest = parseCargoToml(readFileSync(file, "utf8"));
    if (manifest.package.name === r.package) crates.set(r.package, { name: r.package, dir: posix.dirname(`${abs}/${toPosix(r.manifest)}`), manifest });
  }
  const listed = new Map();
  for (const f of nextestFiles) for (const t of readInventoryFile(f).tests) listed.set(t.crate, [...(listed.get(t.crate) ?? []), t]);
  const cache = new Map();
  return {
    has: (name) => crates.has(name),
    manifestFile: (name) => `${crates.get(name).dir.slice(abs.length + 1)}/Cargo.toml`.replace(/^\//, ""),
    inventory(name) {
      if (!cache.has(name)) {
        cache.set(name, listed.has(name) ? { source: "nextest-list", tests: listed.get(name) } : { source: "static-scan", tests: scanCrate(crates.get(name), abs).tests });
      }
      return cache.get(name);
    },
  };
}

export function scriptCatalogEntries(files) {
  const skip = new Set(["README.md", "catalog-extra.txt", "catalog-removed.md"]);
  return [...new Set(files.filter((f) => f.startsWith("scripts/")).map((f) => f.split("/")[1]))].filter((e) => !skip.has(e)).sort();
}

export function makeContext(root, catalogue, { files = listRepoFiles(root), env = process.env } = {}) {
  const fileSet = new Set(files);
  const lists = new Map();
  const nextestFiles = (env.CODETERM_CATALOGUE_NEXTEST_JSON ?? "").split(path.delimiter).filter(Boolean);
  let rust = null;
  return {
    root,
    files,
    fileSet,
    read: (rel) => (fileSet.has(rel) || existsSync(path.join(root, rel)) ? readFileSync(path.join(root, rel), "utf8") : null),
    readList(rel) {
      if (!lists.has(rel)) {
        const list = JSON.parse(readFileSync(path.join(root, rel), "utf8"));
        if (!Array.isArray(list) || !list.every((x) => typeof x === "string")) throw new Error(`${rel} must be a JSON array of paths`);
        lists.set(rel, list);
      }
      return lists.get(rel);
    },
    get rust() {
      rust ??= rustPackages(root, catalogue.runners, nextestFiles);
      return rust;
    },
  };
}

export function discoverRunner(runner, ctx) {
  const d = runner.discovery;
  if (FILE_METHODS.has(d.method)) {
    const owned = d.exclude_list_file ? new Set(ctx.readList(d.exclude_list_file)) : new Set();
    const has = (re) => (f) => new RegExp(re, "m").test(ctx.read(f) ?? "");
    const files = ctx.files
      .filter((f) => matchesAny(f, d.patterns) && !matchesAny(f, d.exclude) && !owned.has(f))
      .filter((f) => (!d.contains || has(d.contains)(f)) && (!d.lacks || !has(d.lacks)(f)));
    const emptyPatterns = d.patterns.filter((p) => !ctx.files.some((f) => globToRegExp(p).test(f)));
    return { source: d.method, files, emptyPatterns };
  }
  if (d.method === "list-file") return { source: "list-file", files: [...ctx.readList(d.list_file)] };
  if (RUST_METHODS.has(d.method)) {
    if (!ctx.rust.has(runner.package)) throw new Error(`Cargo package ${runner.package} is in no workspace and not at ${runner.manifest}`);
    const inv = ctx.rust.inventory(runner.package);
    return { source: inv.source, files: [ctx.rust.manifestFile(runner.package)], tests: inv.tests };
  }
  if (d.method === "script-catalog") return { source: "script-catalog", files: [], members: scriptCatalogEntries(ctx.files) };
  return { source: "single", files: [] };
}

export function discoverAll(catalogue, ctx) {
  return catalogue.runners.map((runner) => {
    try {
      return { runner, ...discoverRunner(runner, ctx) };
    } catch (err) {
      return { runner, source: "error", files: [], error: err.message };
    }
  });
}

const markerKey = (m) => ("binary" in m.target ? `${m.target.binary} ${m.target.test}` : `${m.target.spec} :: ${m.target.title}${m.target.step ? ` [${m.target.step}]` : ""}`);

function overlayErrors(overlay, found, ctx) {
  const errors = [];
  const seen = new Set();
  for (const [kind] of MARKER_CLASSES) {
    overlay[kind].forEach((m, i) => {
      const at = `overlay.${kind}[${i}] ${markerKey(m)}`;
      if (seen.has(markerKey(m))) errors.push(`${at}: duplicate marker`);
      seen.add(markerKey(m));
      const owner = found.find((x) => x.runner.id === m.runner);
      if (!owner) return errors.push(`${at}: runner ${m.runner} is not in the catalogue`);
      if ("binary" in m.target) {
        const pkg = m.target.binary.split("::")[0];
        if (!owner.tests) return errors.push(`${at}: ${m.runner} is not a Rust runner`);
        if (pkg !== owner.runner.package) return errors.push(`${at}: binary ${m.target.binary} is not package ${owner.runner.package}`);
        if (!owner.tests.some((t) => testKey(t) === m.target.test)) errors.push(`${at}: ${m.target.test} is not in the ${owner.source} inventory of ${pkg}`);
        return;
      }
      if (!owner.files.includes(m.target.spec)) return errors.push(`${at}: ${m.target.spec} is not discovered by ${m.runner}`);
      const text = ctx.read(m.target.spec) ?? "";
      if (!text.includes(m.target.title)) errors.push(`${at}: the title is not in ${m.target.spec}`);
      if (m.target.step && !text.includes(m.target.step)) errors.push(`${at}: the step is not in ${m.target.spec}`);
    });
  }
  return errors;
}

function vitestErrors(catalogue, found, ctx, looking, tsUnitRunner) {
  const errors = [];
  const owner = found.find((x) => x.runner.discovery.method === "list-file" && x.runner.discovery.list_file === PATHS.vitest);
  if (!owner) return [`no runner discovers ${PATHS.vitest} with method list-file`];
  if (owner.error) return [`${PATHS.vitest}: ${owner.error}`];
  const list = owner.files;
  const dupes = list.filter((f, i) => list.indexOf(f) !== i);
  if (dupes.length) errors.push(`${PATHS.vitest} lists ${dupes.join(", ")} more than once`);
  for (const f of list) {
    if (!ctx.fileSet.has(f)) errors.push(`${PATHS.vitest} names ${f}, which does not exist`);
    else if (!looking(f)) errors.push(`${PATHS.vitest} names ${f}, which is not a test-looking file`);
  }
  const listed = new Set(list);
  for (const other of found) {
    if (other === owner) continue;
    const overlap = other.files.filter((f) => listed.has(f));
    if (overlap.length) errors.push(`${other.runner.id} also discovers vitest-owned ${overlap.join(", ")}; give it exclude_list_file ${PATHS.vitest}`);
  }
  const tsUnit = found.find((x) => x.runner.id === TS_UNIT_ID);
  if (tsUnitRunner && tsUnit) {
    const theirs = new Set(tsUnitRunner());
    const ours = new Set(tsUnit.files);
    const missing = [...theirs].filter((f) => !ours.has(f));
    const extra = [...ours].filter((f) => !theirs.has(f));
    if (missing.length || extra.length) {
      errors.push(`${TS_UNIT_ID} disagrees with scripts/run-ts-unit-tests.mjs discovery: runner-only ${missing.join(", ") || "none"}; catalogue-only ${extra.join(", ") || "none"}`);
    }
  }
  return errors;
}

export function validateRepo(root, { files, env = process.env, tsUnitRunner } = {}) {
  const errors = [];
  const add = (cls, message) => errors.push({ cls, message });
  const json = (rel) => {
    const file = path.join(root, rel);
    if (!existsSync(file)) return add("invalid-field", `${rel} is missing`), null;
    try {
      return JSON.parse(readFileSync(file, "utf8"));
    } catch (err) {
      return add("invalid-field", `${rel}: ${err.message}`), null;
    }
  };
  const schema = json(PATHS.schema);
  const catalogue = json(PATHS.catalogue);
  const overlay = json(PATHS.overlay);
  if (!schema || !catalogue || !overlay) return { errors, found: [] };
  for (const m of schemaErrors(schema, schema, catalogue, "catalogue")) add("invalid-field", m);
  for (const m of schemaErrors(schema, schema.$defs.overlayFile, overlay, "overlay")) add("invalid-field", m);
  const ids = catalogue.runners?.map((r) => r.id) ?? [];
  for (const id of new Set(ids.filter((id, i) => ids.indexOf(id) !== i))) add("invalid-field", `runner id ${id} is used more than once`);
  if (errors.length) return { errors, found: [], catalogue, overlay };

  const ctx = makeContext(root, catalogue, { files: files ?? listRepoFiles(root), env });
  for (const r of catalogue.runners) {
    if (!ctx.fileSet.has(r.owner_doc)) add("invalid-field", `${r.id}: owner_doc ${r.owner_doc} does not exist`);
    for (const dep of r.depends_on ?? []) if (!ctx.fileSet.has(dep)) add("discovery", `${r.id}: depends_on ${dep} does not exist; the runner imports it`);
  }
  const found = discoverAll(catalogue, ctx);
  for (const f of found) {
    if (f.error) add("discovery", `${f.runner.id}: ${f.error}`);
    for (const p of f.emptyPatterns ?? []) add("discovery", `${f.runner.id}: pattern ${p} matches no file`);
  }
  const vitestOwner = found.find((x) => x.runner.discovery.list_file === PATHS.vitest)?.runner.id;
  const owners = new Map();
  for (const f of found) for (const file of f.files) owners.set(file, [...(owners.get(file) ?? []), f.runner.id]);
  for (const [file, by] of owners) {
    if (by.length > 1 && !by.includes(vitestOwner)) add("double-owned", `${file} is discovered by ${by.join(", ")}; exactly one runner may own it`);
  }
  const { patterns, not_tests: notTests } = catalogue.test_looking;
  const notTestGlobs = notTests.map((n) => n.pattern);
  const looking = (f) => matchesAny(f, patterns) && !matchesAny(f, notTestGlobs);
  for (const file of ctx.files) {
    if (!looking(file)) continue;
    if (!owners.has(file)) add("unmatched", `${file} looks like a test but no runner discovers it`);
    else if (!/\.toml$/.test(file) && !(ctx.read(file) ?? "").replace(/\/\/.*|\/\*[\s\S]*?\*\/|^\s*#.*$/gm, "").trim()) {
      add("empty", `${file} is a test file with nothing in it; it would pass without testing anything`);
    }
  }
  for (const p of notTestGlobs) if (!ctx.files.some((f) => globToRegExp(p).test(f))) add("invalid-field", `test_looking.not_tests pattern ${p} matches no file`);
  for (const m of vitestErrors(catalogue, found, ctx, looking, tsUnitRunner)) add("vitest", m);
  for (const m of overlayErrors(overlay, found, ctx)) add("overlay", m);
  const readme = ctx.read(PATHS.readme);
  if (readme === null || readme.replace(/\r\n/g, "\n") !== renderReadme(catalogue, overlay)) {
    add("readme", `${PATHS.readme} is stale; run node scripts/test-catalogue.mjs readme`);
  }
  return { errors, found, catalogue, overlay };
}

export function selectRunners(catalogue, overlay, { tier, kind } = {}) {
  const matches = (r) => (!tier || r.tier === tier) && (!kind || r.kind === kind);
  const runners = catalogue.runners.filter((r) => matches(r) && (r.status ?? "active") === "active");
  const ids = new Set(runners.map((r) => r.id));
  const mark = (m) => ({ ...m, in_selection: ids.has(m.runner) });
  return {
    tier: tier ?? null,
    kind: kind ?? null,
    runners,
    suspended: catalogue.runners.filter((r) => matches(r) && r.status === "suspended").map((r) => r.id),
    ...Object.fromEntries(MARKER_CLASSES.map(([kind]) => [kind, overlay[kind].map(mark)])),
  };
}

const memberCount = (f) => (f.tests ? `${f.tests.length} tests (${f.source})` : f.members ? `${f.members.length} entries` : f.source === "single" ? "single check" : `${f.files.length} files`);

export function formatSelection(sel, found, format) {
  const byId = new Map(found.map((f) => [f.runner.id, f]));
  if (format === "github-matrix") {
    return JSON.stringify({ include: sel.runners.map((r) => ({ id: r.id, kind: r.kind, runner: r.runner, budget_secs: r.budget_secs })) }) + "\n";
  }
  if (format === "json") {
    const runners = sel.runners.map((r) => {
      const f = byId.get(r.id);
      const members = f.tests ? { source: f.source, test_count: f.tests.length } : { source: f.source, files: f.files };
      return { id: r.id, kind: r.kind, lang: r.lang, tier: r.tier, runner: r.runner, budget_secs: r.budget_secs, needs_instance: r.needs_instance, needs_live_agent: r.needs_live_agent, ...members };
    });
    return JSON.stringify({ ...sel, runners }, null, 2) + "\n";
  }
  const out = [`select tier=${sel.tier ?? "any"} kind=${sel.kind ?? "any"}: ${sel.runners.length} runner(s)`];
  for (const r of sel.runners) out.push(`  ${r.id.padEnd(34)} ${r.kind.padEnd(12)} ${r.tier.padEnd(8)} ${memberCount(byId.get(r.id)).padEnd(30)} ${r.runner}`);
  if (sel.suspended.length) out.push(`suspended (selected by no tier): ${sel.suspended.join(", ")}`);
  return out.join("\n") + "\n" + formatMarkers(sel);
}

export function formatMarkers(overlay) {
  const out = [];
  for (const [kind, label] of MARKER_CLASSES) {
    const markers = overlay[kind];
    out.push(`${label}: ${markers.length}`);
    for (const m of markers) {
      const where = m.in_selection === false ? " (not selected)" : "";
      out.push(`  ${m.runner}${where}: ${markerKey(m)}${m.platforms ? ` on ${m.platforms.join(", ")}` : ""}  [${m.source.commit ?? m.source.task}${m.owner ? `; owner ${m.owner}` : ""}]`);
    }
  }
  return out.join("\n") + "\n";
}

const cell = (s) => String(s).replace(/\|/g, "\\|");
const discoveryText = (d) => {
  const parts = [d.method];
  if (d.patterns) parts.push(d.patterns.map((p) => `\`${p}\``).join(" "));
  if (d.exclude?.length) parts.push(`minus ${d.exclude.map((p) => `\`${p}\``).join(" ")}`);
  if (d.exclude_list_file) parts.push(`minus \`${d.exclude_list_file}\``);
  if (d.list_file) parts.push(`\`${d.list_file}\``);
  if (d.contains) parts.push(`containing \`${d.contains}\``);
  if (d.lacks) parts.push(`lacking \`${d.lacks}\``);
  if (d.fallback) parts.push(`(fallback ${d.fallback})`);
  return parts.join(" ");
};

const runnerText = (r) => {
  const parts = [`\`${r.runner}\``];
  if (r.check) parts.push(`check \`${r.check}\``);
  if (r.depends_on) parts.push(`depends on ${r.depends_on.map((x) => `\`${x}\``).join(", ")}`);
  return parts.join("; ");
};

export function renderReadme(catalogue, overlay) {
  const out = [
    "# tests/",
    "",
    "Generated by `node scripts/test-catalogue.mjs readme` from `catalogue.json` and `catalogue.overlay.json`; do not edit by hand.",
    "The schema is `catalogue.schema.json`. `node scripts/test-catalogue.mjs validate` fails on a test-looking file no runner discovers,",
    "on a file two runners discover, on an empty test file, on an invalid field, on an overlay key that resolves to no test, on a disagreement with",
    "`scripts/vitest-owned-tests.json`, and on a stale README. `select --tier pr|release|manual` lists what a tier runs.",
    "",
    "## Runners",
    "",
    "| Id | Kind | Lang | Tier | Instance | Live agent | Budget (s) | Discovery | Runner | Owner doc |",
    "|---|---|---|---|---|---|---|---|---|---|",
  ];
  for (const r of catalogue.runners) {
    const tier = r.status === "suspended" ? `${r.tier} (suspended)` : r.tier;
    out.push(`| \`${r.id}\` | ${r.kind} | ${r.lang} | ${tier} | ${r.needs_instance ? "yes" : "no"} | ${r.needs_live_agent ? "yes" : "no"} | ${r.budget_secs} | ${cell(discoveryText(r.discovery))} | ${cell(runnerText(r))} | \`${r.owner_doc}\` |`);
  }
  out.push("", "## Test-looking files", "", `Patterns: ${catalogue.test_looking.patterns.map((p) => `\`${p}\``).join(", ")}.`, "", "Not tests:", "");
  for (const n of catalogue.test_looking.not_tests) out.push(`- \`${n.pattern}\`: ${n.reason}`);
  for (const [kind, label] of MARKER_CLASSES) {
    out.push("", `## ${label}`, "");
    if (!overlay[kind].length) {
      out.push("None.");
      continue;
    }
    out.push("| Runner | Target | Platforms | Reason | Source | Owner |", "|---|---|---|---|---|---|");
    for (const m of overlay[kind]) {
      const source = [m.source.commit && `commit ${m.source.commit}`, m.source.task, m.fix_task && `fix ${m.fix_task}`].filter(Boolean).join(", ");
      out.push(`| \`${m.runner}\` | ${cell(markerKey(m))} | ${m.platforms?.join(", ") ?? "all"} | ${cell(m.reason)} | ${source} | ${cell(m.owner ?? "")} |`);
    }
  }
  return out.join("\n") + "\n";
}

const UNITTEST_DRIVER = `
import sys, unittest
files, skip, args = [], set(), iter(sys.argv[1:])
for a in args:
    (skip.add(next(args)) if a == "--skip" else files.append(a))
def flat(s):
    for t in s:
        yield from (flat(t) if isinstance(t, unittest.TestSuite) else [t])
suite, empty = unittest.TestSuite(), []
for f in files:
    tests = list(flat(unittest.defaultTestLoader.loadTestsFromName(f[:-3].replace("/", "."))))
    if not tests: empty.append(f)
    for t in tests:
        if t.id().rsplit(".", 1)[-1] in skip: print("skip (overlay):", t.id())
        else: suite.addTest(t)
for f in empty: print("ERROR: no unittest test cases in", f)
ok = unittest.TextTestRunner(verbosity=1).run(suite).wasSuccessful()
sys.exit(0 if ok and not empty else 1)
`;

const resolvePython = () => (spawnSync("python3", ["-c", ""], { stdio: "ignore" }).status === 0 ? "python3" : "python");

// Each command is an argv; the caller spawns them in order and fails when any fails.
export function runCommands(runner, files, overlay, { platform = PLATFORM, python = "python3", node = process.execPath } = {}) {
  if (!runner.exec) throw new Error(`${runner.id} has no exec; run its runner command: ${runner.runner}`);
  if (!files.length) throw new Error(`${runner.id} discovered no files`);
  const stale = [...overlay.stale, ...overlay.platform_red].filter((m) => m.runner === runner.id && "spec" in m.target && (!m.platforms || m.platforms.includes(platform)));
  let [bin, ...args] = runner.exec;
  if (bin === "node") bin = node;
  if (bin === "python3") bin = python;
  const pyUnittest = args[0] === "-m" && args[1] === "unittest";
  if (pyUnittest) {
    const skips = stale.flatMap((m) => ["--skip", m.target.title]);
    return (runner.exec_each ? files.map((f) => [f]) : [files]).map((batch) => [bin, "-c", UNITTEST_DRIVER, ...batch, ...skips]);
  }
  const skips = bin === node && args.includes("--test") ? stale.map((m) => `--test-skip-pattern=^${escapeRe(m.target.title)}$`) : [];
  return (runner.exec_each ? files.map((f) => [f]) : [files]).map((batch) => [bin, ...args, ...skips, ...batch]);
}

function argValue(args, flag) {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
}

function load(root) {
  const read = (rel) => JSON.parse(readFileSync(path.join(root, rel), "utf8"));
  const catalogue = read(PATHS.catalogue);
  return { catalogue, overlay: read(PATHS.overlay), ctx: makeContext(root, catalogue) };
}

async function tsUnitRunnerFor(root) {
  if (!existsSync(path.join(root, "src")) || !existsSync(path.join(root, "packages"))) return undefined;
  const { discoverTests } = await import("./run-ts-unit-tests.mjs");
  return () => discoverTests({}, root);
}

export async function main(args) {
  const cmd = args[0];
  const root = path.resolve(argValue(args, "--root") ?? path.join(path.dirname(fileURLToPath(import.meta.url)), ".."));
  const entryOf = (catalogue) => {
    const id = argValue(args, "--entry");
    const runner = catalogue.runners.find((r) => r.id === id);
    if (!runner) throw new Error(`--entry ${id ?? "<missing>"} is not a catalogue runner`);
    return runner;
  };
  if (cmd === "validate") {
    const { errors, found } = validateRepo(root, { tsUnitRunner: await tsUnitRunnerFor(root) });
    for (const f of found) process.stdout.write(`  ${f.runner.id.padEnd(34)} ${memberCount(f)}\n`);
    for (const e of errors) process.stderr.write(`ERROR [${e.cls}] ${e.message}\n`);
    const owned = found.reduce((n, f) => n + f.files.length, 0);
    process.stdout.write(`validate: ${found.length} runners, ${owned} owned files, ${errors.length} error(s)\n`);
    return errors.length ? 1 : 0;
  }
  if (cmd === "readme") {
    const { catalogue, overlay } = load(root);
    const text = renderReadme(catalogue, overlay);
    if (args.includes("--check")) {
      const have = existsSync(path.join(root, PATHS.readme)) ? readFileSync(path.join(root, PATHS.readme), "utf8").replace(/\r\n/g, "\n") : null;
      if (have === text) return 0;
      process.stderr.write(`ERROR: ${PATHS.readme} is stale; run node scripts/test-catalogue.mjs readme\n`);
      return 1;
    }
    writeFileSync(path.join(root, PATHS.readme), text);
    return 0;
  }
  if (cmd === "stale") {
    const { overlay } = load(root);
    process.stdout.write(formatMarkers(overlay));
    return 0;
  }
  if (cmd === "select") {
    const { catalogue, overlay, ctx } = load(root);
    const tier = argValue(args, "--tier");
    const format = argValue(args, "--format") ?? "text";
    if ((tier && !["pr", "release", "manual"].includes(tier)) || !["text", "json", "github-matrix"].includes(format)) {
      process.stderr.write("usage: select [--tier pr|release|manual] [--kind K] [--format text|json|github-matrix]\n");
      return 2;
    }
    const sel = selectRunners(catalogue, overlay, { tier, kind: argValue(args, "--kind") });
    const found = discoverAll({ ...catalogue, runners: sel.runners }, ctx);
    for (const f of found) if (f.error) throw new Error(`${f.runner.id}: ${f.error}`);
    process.stdout.write(formatSelection(sel, found, format));
    return 0;
  }
  if (cmd === "list" || cmd === "run") {
    const { catalogue, overlay, ctx } = load(root);
    const runner = entryOf(catalogue);
    const { files } = discoverRunner(runner, ctx);
    if (cmd === "list") {
      process.stdout.write(files.map((f) => `${f}\n`).join(""));
      return 0;
    }
    let status = 0;
    for (const argv of runCommands(runner, files, overlay, { python: resolvePython() })) {
      process.stdout.write(`== ${runner.id}: ${files.length} discovered file(s)\n`);
      const r = spawnSync(argv[0], argv.slice(1), { cwd: root, stdio: "inherit" });
      if (r.error) throw r.error;
      if (r.status !== 0) status = 1;
    }
    return status;
  }
  process.stderr.write("usage: node scripts/test-catalogue.mjs validate | select | list --entry ID | run --entry ID | stale | readme [--check]\n");
  return 2;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => (process.exitCode = code),
    (err) => {
      process.stderr.write(`test-catalogue: ${err.message}\n`);
      process.exitCode = 2;
    },
  );
}

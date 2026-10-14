#!/usr/bin/env node
// Caller: crate-split QA (static vs nextest test-count oracle); scripts tests/rust-test-inventory.test.mjs
// Static Rust test inventory, resolved the way rustc resolves modules; never runs cargo.
//
// Scan: node scripts/rust-test-inventory.mjs [--root <repo>] [--out <file>]
//   {"tests": [{"crate", "module_path", "test_name"}], "warnings": [{"kind", "crate", "file", "line", "message"}]}
//   tests are sorted by crate, module_path, test_name; module_path is "" at a target root and uses "::".
//   A test appears once per test binary that compiles it. Warning kinds: macro-body, nested-item,
//   unreachable-file, cfg-gated, unknown-test-attr, unresolved-module, unresolved-include, parse-error, target-skipped.
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const posix = path.posix;
const OPEN = { "{": "}", "(": ")", "[": "]" };
const CLOSE = new Set(["}", ")", "]"]);
const PUNCT2 = new Set(["::", "!=", "==", "=>", "->", "<=", ">=", "&&", "||", ".."]);
const isIdStart = (c) => c !== undefined && (/[A-Za-z_]/.test(c) || c.charCodeAt(0) > 0x7f);
const isIdChar = (c) => c !== undefined && (/[A-Za-z0-9_]/.test(c) || c.charCodeAt(0) > 0x7f);

export function lexRust(src) {
  const toks = [];
  const n = src.length;
  let i = 0;
  let line = 1;
  const countLines = (from, to) => {
    for (let k = from; k < to; k++) if (src[k] === "\n") line++;
  };
  const quoted = (start, open) => {
    let j = open + 1;
    while (j < n && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
    if (j >= n) throw new Error(`unterminated string at line ${line}`);
    const raw = src.slice(open + 1, j);
    toks.push({ k: "str", v: raw.replace(/\\(.)/g, "$1"), text: src.slice(start, j + 1), line });
    countLines(start, j + 1);
    return j + 1;
  };
  const rawQuoted = (start, hashStart) => {
    let j = hashStart;
    while (src[j] === "#") j++;
    const close = '"' + "#".repeat(j - hashStart);
    const end = src.indexOf(close, j + 1);
    if (end < 0) throw new Error(`unterminated raw string at line ${line}`);
    toks.push({ k: "str", v: src.slice(j + 1, end), text: src.slice(start, end + close.length), line });
    countLines(start, end + close.length);
    return end + close.length;
  };
  const charLit = (start, quote) => {
    let j = quote + 1;
    if (src[j] === "\\") {
      j += 2;
      while (j < n && src[j] !== "'" && src[j] !== "\n") j++;
    } else {
      j += src.codePointAt(j) > 0xffff ? 2 : 1;
    }
    if (src[j] !== "'") throw new Error(`unterminated char literal at line ${line}`);
    toks.push({ k: "char", v: src.slice(start, j + 1), text: src.slice(start, j + 1), line });
    return j + 1;
  };
  const isCharAt = (q) => {
    if (src[q + 1] === "\\") return true;
    const width = src.codePointAt(q + 1) > 0xffff ? 2 : 1;
    return src[q + 1] !== undefined && src[q + 1] !== "\n" && src[q + 1 + width] === "'";
  };
  while (i < n) {
    const c = src[i];
    if (c === "\n") {
      line++;
      i++;
    } else if (/\s/.test(c)) {
      i++;
    } else if (c === "/" && src[i + 1] === "/") {
      while (i < n && src[i] !== "\n") i++;
    } else if (c === "/" && src[i + 1] === "*") {
      let depth = 1;
      i += 2;
      while (i < n && depth > 0) {
        if (src[i] === "/" && src[i + 1] === "*") {
          depth++;
          i += 2;
        } else if (src[i] === "*" && src[i + 1] === "/") {
          depth--;
          i += 2;
        } else {
          if (src[i] === "\n") line++;
          i++;
        }
      }
      if (depth > 0) throw new Error("unterminated block comment");
    } else if (c === '"') {
      i = quoted(i, i);
    } else if (c === "'") {
      if (isCharAt(i)) {
        i = charLit(i, i);
      } else {
        let j = i + 1;
        while (isIdChar(src[j])) j++;
        toks.push({ k: "lifetime", v: src.slice(i, j), line });
        i = j;
      }
    } else if (isIdStart(c)) {
      const lit = /^(?:(b|c)?r(#*)"|(b|c)"|b')/.exec(src.slice(i, i + 3 + 64));
      if (lit && lit[0].endsWith('"') && lit[0].includes("r")) {
        i = rawQuoted(i, i + lit[0].indexOf("r") + 1);
      } else if (lit && lit[0].endsWith('"')) {
        i = quoted(i, i + 1);
      } else if (lit && lit[0] === "b'") {
        i = charLit(i, i + 1);
      } else if (c === "r" && src[i + 1] === "#" && isIdStart(src[i + 2])) {
        let j = i + 2;
        while (isIdChar(src[j])) j++;
        toks.push({ k: "ident", v: src.slice(i, j), raw: true, line });
        i = j;
      } else {
        let j = i;
        while (isIdChar(src[j])) j++;
        toks.push({ k: "ident", v: src.slice(i, j), line });
        i = j;
      }
    } else if (/[0-9]/.test(c)) {
      let j = i;
      while (isIdChar(src[j]) || (src[j] === "." && /[0-9]/.test(src[j + 1] ?? ""))) j++;
      toks.push({ k: "num", v: src.slice(i, j), line });
      i = j;
    } else {
      const two = src.slice(i, i + 2);
      const v = PUNCT2.has(two) ? two : c;
      toks.push({ k: "punct", v, line });
      i += v.length;
    }
  }
  return toks;
}

const tokText = (ts) =>
  ts
    .map((t) => t.text ?? t.v)
    .join(" ")
    .replace(/ ?(::|\(|\)|,|=) ?/g, "$1")
    .replace(/,/g, ", ");

export function classifyAttr(ts) {
  let k = 0;
  const segs = [];
  while (k < ts.length && (ts[k].k === "ident" || ts[k].v === "::")) {
    if (ts[k].k === "ident") segs.push(ts[k].v);
    k++;
  }
  const p = segs.join("::");
  const attr = {};
  if (p === "test" || p === "tokio::test") attr.test = true;
  else if (segs.length > 1 && segs[segs.length - 1] === "test") attr.unknownTest = p;
  if (p === "cfg" && ts[k]?.v === "(") attr.cfg = tokText(ts.slice(k + 1, -1));
  if (p === "path" && ts[k]?.v === "=" && ts[k + 1]?.k === "str") attr.path = ts[k + 1].v;
  if (p === "cfg_attr") {
    const rest = ts.slice(k);
    const pathy = rest.some((t, j) => t.v === "path" && rest[j + 1]?.v === "=");
    const testy = rest.some((t, j) => t.v === "test" && rest[j - 1]?.v === "," && rest[j + 1]?.v !== "(");
    if (pathy || testy) attr.cfgAttrHazard = tokText(rest.slice(1, -1));
  }
  return attr;
}

function matchClose(toks, open) {
  let depth = 0;
  for (let j = open; j < toks.length; j++) {
    if (OPEN[toks[j].v] && toks[j].k === "punct") depth++;
    else if (CLOSE.has(toks[j].v) && toks[j].k === "punct" && --depth === 0) return j;
  }
  return -1;
}

const isTestCfg = (cfg) => cfg === "test";

export function parseRustItems(src) {
  const out = { tests: [], mods: [], includes: [], warnings: [], innerCfgs: [] };
  let toks;
  try {
    toks = lexRust(src);
  } catch (err) {
    out.warnings.push({ kind: "parse-error", line: 0, message: err.message });
    return out;
  }
  const root = { kind: "mod", delim: null, cfgs: out.innerCfgs, macro: false };
  const stack = [root];
  let attrs = [];
  let pending = null;
  const warn = (kind, line, message) => out.warnings.push({ kind, line, message });
  const inlineMods = () => stack.filter((f) => f.kind === "mod" && f.delim).map((f) => ({ name: f.name, path: f.pathAttr ?? null }));
  const allMods = () => stack.every((f) => f.kind === "mod");
  const scopeCfgs = () => stack.flatMap((f) => f.cfgs);
  const ownCfgs = (list) => list.filter((a) => a.cfg && !isTestCfg(a.cfg)).map((a) => a.cfg);
  for (let i = 0; i < toks.length; i++) {
    const t = toks[i];
    const top = stack[stack.length - 1];
    if (t.k === "punct" && t.v === "#") {
      const inner = toks[i + 1]?.v === "!" && toks[i + 2]?.v === "[";
      const open = inner ? i + 2 : i + 1;
      if (toks[open]?.v === "[") {
        const end = matchClose(toks, open);
        if (end < 0) {
          warn("parse-error", t.line, "unbalanced attribute");
          return out;
        }
        const attr = { ...classifyAttr(toks.slice(open + 1, end)), line: t.line };
        i = end;
        if (top.macro) {
          if (attr.test || attr.unknownTest) warn("macro-body", t.line, "test attribute inside a macro body; its expansion is not resolved statically");
        } else if (inner) {
          if (attr.cfg && !isTestCfg(attr.cfg)) top.cfgs.push(attr.cfg);
          if (attr.cfgAttrHazard) warn("cfg-gated", t.line, `cfg_attr(${attr.cfgAttrHazard}) changes module resolution or test status`);
        } else {
          attrs.push(attr);
        }
        continue;
      }
    }
    if (t.k === "ident" && !t.raw) {
      const next = toks[i + 1];
      if (t.v === "macro_rules" && next?.v === "!") {
        pending = "macro";
        attrs = [];
        i++;
        continue;
      }
      if (next?.v === "!" && next.k === "punct" && toks[i + 2]?.k === "punct" && OPEN[toks[i + 2].v]) {
        if (t.v === "include" && !top.macro) {
          const lit = toks[i + 3];
          if (toks[i + 2].v === "(" && lit?.k === "str" && toks[i + 4]?.v === ")" && allMods()) {
            out.includes.push({ file: lit.v, line: t.line, inline: inlineMods(), cfgs: [...scopeCfgs(), ...ownCfgs(attrs)] });
            attrs = [];
            i += 4;
            continue;
          }
          warn("unresolved-include", t.line, "include! with a computed path or outside module scope is not scanned");
        }
        pending = "macro";
        attrs = [];
        i++;
        continue;
      }
      if (t.v === "fn" && next?.k === "ident") {
        const testAttr = attrs.find((a) => a.test);
        for (const a of attrs.filter((x) => x.unknownTest)) warn("unknown-test-attr", a.line, `#[${a.unknownTest}] on fn ${next.v} is not a recognised test attribute`);
        for (const a of attrs.filter((x) => x.cfgAttrHazard)) warn("cfg-gated", a.line, `cfg_attr(${a.cfgAttrHazard}) on fn ${next.v}`);
        if (testAttr && top.macro) {
          warn("macro-body", testAttr.line, `test fn ${next.v} inside a macro body`);
        } else if (testAttr && !allMods()) {
          warn("nested-item", testAttr.line, `test fn ${next.v} is nested inside a non-module item; libtest cannot name it`);
        } else if (testAttr) {
          out.tests.push({ name: next.v, line: testAttr.line, inline: inlineMods(), cfgs: [...scopeCfgs(), ...ownCfgs(attrs)] });
        }
        pending = "fn";
        attrs = [];
        i++;
        continue;
      }
      if (t.v === "mod" && next?.k === "ident" && (toks[i + 2]?.v === ";" || toks[i + 2]?.v === "{")) {
        const pathAttr = attrs.find((a) => a.path !== undefined)?.path ?? null;
        for (const a of attrs.filter((x) => x.cfgAttrHazard)) warn("cfg-gated", a.line, `cfg_attr(${a.cfgAttrHazard}) on mod ${next.v}`);
        const cfgs = ownCfgs(attrs);
        attrs = [];
        if (toks[i + 2].v === ";") {
          if (!top.macro && allMods()) out.mods.push({ name: next.v, line: t.line, pathAttr, inline: inlineMods(), cfgs: [...scopeCfgs(), ...cfgs] });
          else if (!top.macro) warn("unresolved-module", t.line, `mod ${next.v}; declared inside a non-module item`);
        } else {
          stack.push({ kind: allMods() ? "mod" : "other", delim: "{", name: next.v, pathAttr, cfgs, macro: top.macro });
        }
        i += 2;
        continue;
      }
    }
    if (t.k === "punct" && OPEN[t.v]) {
      const kind = pending === "fn" && t.v === "{" ? "fn" : "other";
      stack.push({ kind, delim: t.v, cfgs: [], macro: top.macro || pending === "macro" });
      if (pending === "macro" || t.v === "{") pending = null;
      if (t.v === "{") attrs = [];
      continue;
    }
    if (t.k === "punct" && CLOSE.has(t.v)) {
      const f = stack.pop();
      if (!f?.delim || OPEN[f.delim] !== t.v) {
        warn("parse-error", t.line, `unbalanced '${t.v}'`);
        return out;
      }
      if (t.v === "}") attrs = [];
      continue;
    }
    if (t.v === ";" || t.v === ",") {
      attrs = [];
      if (t.v === ";") pending = null;
    }
  }
  if (stack.length !== 1) warn("parse-error", toks[toks.length - 1]?.line ?? 0, "unclosed delimiter at end of file");
  return out;
}

const stripComment = (line) => {
  let inStr = false;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === '"' && line[i - 1] !== "\\") inStr = !inStr;
    else if (line[i] === "#" && !inStr) return line.slice(0, i);
  }
  return line;
};

function tomlValue(raw) {
  const v = raw.trim();
  if (v.startsWith('"')) return JSON.parse(v.slice(0, v.indexOf('"', 1) + 1));
  if (v.startsWith("'")) return v.slice(1, v.indexOf("'", 1));
  if (v === "true" || v === "false") return v === "true";
  if (v.startsWith("[")) return [...v.matchAll(/"((?:[^"\\]|\\.)*)"|'([^']*)'/g)].map((m) => m[1] ?? m[2]);
  return undefined;
}

export function parseCargoToml(text) {
  const manifest = { package: {}, workspace: { members: [], exclude: [] }, lib: null, bins: [], tests: [] };
  let table = null;
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    let line = stripComment(lines[i]).trim();
    if (!line) continue;
    const header = /^(\[\[?)(.*?)\]\]?$/.exec(line);
    if (header) {
      const name = header[2].trim();
      if (header[1] === "[[" && (name === "bin" || name === "test")) {
        table = {};
        manifest[name === "bin" ? "bins" : "tests"].push(table);
      } else if (name === "lib") {
        table = manifest.lib = {};
      } else if (name === "package" || name === "workspace") {
        table = manifest[name];
      } else {
        table = null;
      }
      continue;
    }
    const kv = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/.exec(line);
    if (!kv || !table) continue;
    let raw = kv[2];
    if (raw.startsWith("[")) {
      while (!/\]\s*$/.test(raw) && i + 1 < lines.length) raw += " " + stripComment(lines[++i]).trim();
    }
    const value = tomlValue(raw);
    if (value !== undefined) table[kv[1]] = value;
  }
  return manifest;
}

export const nodeFs = {
  read: (p) => {
    try {
      return readFileSync(p, "utf8");
    } catch {
      return null;
    }
  },
  isFile: (p) => existsSync(p) && statSync(p).isFile(),
  isDir: (p) => existsSync(p) && statSync(p).isDirectory(),
  list: (dir) => {
    try {
      return readdirSync(dir, { withFileTypes: true }).map((d) => ({ name: d.name, dir: d.isDirectory() }));
    } catch {
      return [];
    }
  },
};

const norm = (p) => posix.normalize(p.replace(/\\/g, "/"));

function listRs(fs, dir) {
  const files = [];
  for (const e of fs.list(dir)) {
    const p = posix.join(dir, e.name);
    if (e.dir) files.push(...listRs(fs, p));
    else if (e.name.endsWith(".rs")) files.push(p);
  }
  return files;
}

export function crateTargets(dir, manifest, fs = nodeFs) {
  const targets = [];
  const skipped = [];
  const seen = new Set();
  const add = (kind, rel, spec = {}) => {
    const file = norm(posix.join(dir, rel));
    if (seen.has(file) || !fs.isFile(file)) return;
    seen.add(file);
    if (spec.test === false || spec.harness === false) skipped.push({ kind, file, reason: spec.test === false ? "test = false" : "harness = false" });
    else targets.push({ kind, file });
  };
  add("lib", manifest.lib?.path ?? "src/lib.rs", manifest.lib ?? {});
  for (const b of manifest.bins) add("bin", b.path ?? (b.name ? `src/bin/${b.name}.rs` : "src/main.rs"), b);
  for (const t of manifest.tests) add("test", t.path ?? `tests/${t.name}.rs`, t);
  if (manifest.package.autobins !== false) {
    add("bin", "src/main.rs");
    for (const e of fs.list(posix.join(dir, "src/bin"))) add("bin", e.dir ? `src/bin/${e.name}/main.rs` : `src/bin/${e.name}`);
  }
  if (manifest.package.autotests !== false) {
    for (const e of fs.list(posix.join(dir, "tests"))) add("test", e.dir ? `tests/${e.name}/main.rs` : `tests/${e.name}`);
  }
  return { targets: targets.filter((t) => t.file.endsWith(".rs")), skipped };
}

export function workspaceCrates(root, fs = nodeFs) {
  const crates = [];
  const warnings = [];
  const seen = new Set();
  const load = (dir) => {
    dir = norm(dir);
    if (seen.has(dir)) return null;
    seen.add(dir);
    const text = fs.read(posix.join(dir, "Cargo.toml"));
    if (text === null) {
      warnings.push({ kind: "unresolved-module", crate: "", file: posix.join(dir, "Cargo.toml"), line: 0, message: "workspace member manifest is missing" });
      return null;
    }
    const manifest = parseCargoToml(text);
    if (manifest.package.name) crates.push({ name: manifest.package.name, dir, manifest });
    return manifest;
  };
  for (const ws of ["src-tauri", "cli"]) {
    const wsDir = norm(posix.join(root, ws));
    const manifest = load(wsDir);
    for (const member of manifest?.workspace.members ?? []) {
      if (member.endsWith("/*")) {
        const base = posix.join(wsDir, member.slice(0, -2));
        for (const e of fs.list(base)) if (e.dir && !manifest.workspace.exclude.includes(`${member.slice(0, -2)}/${e.name}`)) load(posix.join(base, e.name));
      } else if (member !== ".") {
        load(posix.join(wsDir, member));
      }
    }
  }
  return { crates, warnings };
}

export function scanCrate(crate, root, fs = nodeFs) {
  const tests = [];
  const warnings = [];
  const gated = new Map();
  const reached = new Set();
  const rel = (p) => (p.startsWith(root + "/") ? p.slice(root.length + 1) : p);
  const warn = (kind, file, line, message) => warnings.push({ kind, crate: crate.name, file: rel(file), line, message });
  const exists = (p) => fs.isFile(p);

  const walk = (file, modPath, ctx, cfgs, active, included) => {
    if (active.includes(file)) return warn("unresolved-module", file, 0, "module cycle through #[path]");
    const src = fs.read(file);
    if (src === null) return warn("unresolved-module", file, 0, "module file is missing");
    reached.add(file);
    const items = parseRustItems(src);
    const fileCfgs = [...cfgs, ...items.innerCfgs];
    for (const w of items.warnings) warn(w.kind, file, w.line, w.message);
    const segsOf = (inline) => [...modPath, ...inline.map((m) => m.name)];
    const ctxOf = (inline) => {
      let c = ctx;
      for (const m of inline) {
        c = m.path !== null ? { dir: norm(posix.join(c.dir, m.path)), relative: null } : { dir: posix.join(c.dir, ...(c.relative ? [c.relative] : []), m.name), relative: null };
      }
      return c;
    };
    for (const t of items.tests) {
      const module_path = segsOf(t.inline).join("::");
      tests.push({ crate: crate.name, module_path, test_name: t.name });
      const gates = [...new Set([...cfgs, ...t.cfgs])];
      if (gates.length) {
        const key = `${module_path}\u0000${gates.join(" && ")}`;
        const g = gated.get(key) ?? { file, line: t.line, module_path, cfg: gates.join(" && "), count: 0 };
        g.count++;
        gated.set(key, g);
      }
    }
    for (const m of items.mods) {
      if (included) {
        warn("unresolved-module", file, m.line, `mod ${m.name}; inside an include!d file is not resolved`);
        continue;
      }
      const c = ctxOf(m.inline);
      let child;
      let childCtx;
      if (m.pathAttr !== null) {
        child = norm(posix.join(c.dir, m.pathAttr));
        childCtx = { dir: posix.dirname(child), relative: null };
      } else {
        const base = posix.join(c.dir, ...(c.relative ? [c.relative] : []));
        const flat = posix.join(base, `${m.name}.rs`);
        const nested = posix.join(base, m.name, "mod.rs");
        if (exists(flat) && exists(nested)) {
          warn("unresolved-module", file, m.line, `mod ${m.name}; is ambiguous between ${rel(flat)} and ${rel(nested)}`);
          continue;
        }
        if (exists(flat)) [child, childCtx] = [flat, { dir: base, relative: m.name.replace(/^r#/, "") }];
        else if (exists(nested)) [child, childCtx] = [nested, { dir: posix.join(base, m.name), relative: null }];
        else {
          warn("unresolved-module", file, m.line, `mod ${m.name}; has no file at ${rel(flat)} or ${rel(nested)}`);
          continue;
        }
      }
      walk(child, segsOf([...m.inline, m]), childCtx, [...fileCfgs, ...m.cfgs.filter((x) => !fileCfgs.includes(x))], [...active, file], false);
    }
    for (const inc of items.includes) {
      const target = norm(posix.join(posix.dirname(file), inc.file));
      walk(target, segsOf(inc.inline), ctxOf(inc.inline), [...fileCfgs, ...inc.cfgs.filter((x) => !fileCfgs.includes(x))], [...active, file], true);
    }
  };

  const { targets, skipped } = crateTargets(crate.dir, crate.manifest, fs);
  for (const s of skipped) warn("target-skipped", s.file, 0, `${s.kind} target has ${s.reason}; its tests are not listed by libtest`);
  for (const target of targets) walk(target.file, [], { dir: posix.dirname(target.file), relative: null }, [], [], false);
  for (const g of gated.values()) warn("cfg-gated", g.file, g.line, `${g.count} test(s) in ${g.module_path || "<root>"} compile only under cfg(${g.cfg})`);
  for (const sub of ["src", "tests"]) {
    for (const file of listRs(fs, posix.join(crate.dir, sub))) {
      if (reached.has(file) || skipped.some((s) => s.file === file)) continue;
      const src = fs.read(file) ?? "";
      const hasTests = /#\s*\[\s*(?:tokio\s*::\s*)?test\b/.test(src);
      warn("unreachable-file", file, 0, `no module declaration reaches this file${hasTests ? "; it contains test attributes" : ""}`);
    }
  }
  return { tests, warnings };
}

const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
export const compareTests = (a, b) => cmp(a.crate, b.crate) || cmp(a.module_path, b.module_path) || cmp(a.test_name, b.test_name);
const compareWarnings = (a, b) => cmp(a.crate, b.crate) || cmp(a.file, b.file) || a.line - b.line || cmp(a.kind, b.kind) || cmp(a.message, b.message);

export function scanWorkspace(rootDir, fs = nodeFs) {
  const root = norm(path.resolve(rootDir));
  const { crates, warnings } = workspaceCrates(root, fs);
  const tests = [];
  for (const crate of crates) {
    const r = scanCrate(crate, root, fs);
    tests.push(...r.tests);
    warnings.push(...r.warnings);
  }
  return { tests: tests.sort(compareTests), warnings: warnings.sort(compareWarnings), crates: crates.map((c) => c.name) };
}

export function formatInventory(inv) {
  const lines = (xs) => xs.map((x) => "    " + JSON.stringify(x)).join(",\n");
  return `{\n  "tests": [\n${lines(inv.tests)}\n  ],\n  "warnings": [\n${lines(inv.warnings)}\n  ]\n}\n`;
}

function argValue(args, flag) {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
}

function main(args) {
  const root = argValue(args, "--root") ?? path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
  const out = argValue(args, "--out");
  const inv = scanWorkspace(root);
  const text = formatInventory(inv);
  if (out) writeFileSync(out, text);
  else process.stdout.write(text);
  process.stderr.write(`crates: ${inv.crates.length}  tests: ${inv.tests.length}  warnings: ${inv.warnings.length}\n`);
  for (const w of inv.warnings.slice(0, 10)) process.stderr.write(`  ${w.kind} ${w.file}:${w.line} ${w.message}\n`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}

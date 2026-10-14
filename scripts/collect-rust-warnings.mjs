import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const stripAnsi = text => text.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '');
export const warningKey = w => JSON.stringify([w.target, w.file, w.line, w.code, w.message]);
export function deduplicate(warnings) {
  return [...new Map(warnings.map(w => [warningKey(w), w])).values()].sort((a, b) => warningKey(a).localeCompare(warningKey(b)));
}
export function normalizeFile(file, root = process.cwd()) {
  let normalized = file.replaceAll('\\', '/');
  const prefix = root.replaceAll('\\', '/').replace(/\/$/, '') + '/';
  if (normalized.startsWith(prefix)) normalized = normalized.slice(prefix.length);
  normalized = normalized.replace(/^\/src\//, '').replace(/^\.\//, '');
  if (normalized.startsWith('src/service/') || normalized.startsWith('src/agent/')) normalized = 'src-tauri/' + normalized;
  return normalized;
}
export function parseWarnings(input, { platform, target, crate = 'unknown', root } = {}) {
  const records = [];
  const text = [];
  for (const line of stripAnsi(input).split(/\r?\n/)) {
    let item;
    try { item = JSON.parse(line); } catch { text.push(line); continue; }
    if (item?.reason !== 'compiler-message' || item.message?.level !== 'warning') continue;
    const diagnostic = item.message;
    const span = diagnostic.spans?.find(s => s.is_primary) || diagnostic.spans?.[0];
    records.push({ platform, target, crate: item.target?.name || item.package_id || crate,
      file: normalizeFile(span?.file_name || '<unknown>', root), line: span?.line_start || 0,
      code: diagnostic.code?.code || 'unknown', message: diagnostic.message,
      rendered: stripAnsi(diagnostic.rendered || diagnostic.message) });
  }
  let currentCrate = crate;
  const plain = text.join('\n');
  const blocks = plain.split(/(?=^\s*warning(?:\[[^\]]+\])?:)/m);
  for (const block of blocks) {
    const compiling = [...block.matchAll(/^\s*(?:Checking|Compiling) ([\w-]+)/gm)];
    const header = block.match(/^\s*warning(?:\[([^\]]+)\])?:\s*(.+)/);
    if (header && !/^(?:`[^`]+`.*generated \d+ warning|\d+ warnings? emitted)/.test(header[2])) {
      const location = block.match(/(?:-->|:::)\s+(.+?):(\d+):(\d+)/);
      const lint = header[1] || block.match(/#\[warn\(([^)]+)\)\]/)?.[1] ||
        (/unused imports?/.test(header[2]) ? 'unused_imports' : /does not need to be mutable/.test(header[2]) ? 'unused_mut' : /never used/.test(header[2]) ? 'dead_code' : 'unknown');
      records.push({ platform, target, crate: currentCrate, file: normalizeFile(location?.[1] || '<unknown>', root),
        line: Number(location?.[2] || 0), code: lint, message: header[2].trim(),
        rendered: block.split(/^\s*(?:Compiling|Checking|Finished) /m)[0].trim() });
    }
    if (compiling.length) currentCrate = compiling.at(-1)[1];
  }
  return deduplicate(records);
}
export function counts(warnings, field) {
  const result = {};
  for (const w of warnings) result[w[field]] = (result[w[field]] || 0) + 1;
  return Object.fromEntries(Object.entries(result).sort(([a], [b]) => a.localeCompare(b)));
}
export function makeBaseline(warnings, targets) {
  const entries = deduplicate(warnings).map(({ target, file, line, code, message }) => ({ target, file, line, code, message }));
  return { version: 1, initialized: true, targets: [...new Set(targets)].sort(), counts: { ...Object.fromEntries(targets.map(t => [t, 0])), ...counts(entries, 'target') }, warnings: entries };
}
export function ratchet(warnings, baseline, targets) {
  if (!baseline.initialized) return { bootstrap: true, added: [], removed: [] };
  const allowed = new Set(baseline.warnings.map(warningKey));
  const observed = new Set(warnings.map(warningKey));
  return { bootstrap: false, added: warnings.filter(w => !allowed.has(warningKey(w))),
    removed: baseline.warnings.filter(w => targets.includes(w.target) && !observed.has(warningKey(w))) };
}
export function assertBaselineShrinks(baseline, previous) {
  if (!previous.initialized) return;
  if (!baseline.initialized) throw new Error('Cannot reset an initialized warnings baseline');
  const allowed = new Set(previous.warnings.map(warningKey));
  if (baseline.warnings.some(w => !allowed.has(warningKey(w)))) throw new Error('Warning baseline may only shrink');
  for (const [target, count] of Object.entries(baseline.counts)) {
    if (count > (previous.counts[target] || 0)) throw new Error('Warning counts may only shrink');
  }
  const actual = makeBaseline(baseline.warnings, baseline.targets);
  const sortedCounts = counts => JSON.stringify(Object.entries(counts).sort(([a], [b]) => a.localeCompare(b)));
  if (sortedCounts(actual.counts) !== sortedCounts(baseline.counts)) throw new Error('Baseline counts do not match warning identities');
}
export function renderReport(warnings, targets, decision) {
  const escape = s => String(s).replaceAll('|', '\\|').replaceAll('\n', ' ');
  const table = (title, rows) => `\n### ${title}\n\n| ${title} | Warnings |\n| --- | ---: |\n${Object.entries(rows).map(([key, count]) => `| ${escape(key)} | ${count} |`).join('\n')}\n`;
  return '# Rust warnings\n\n' + warnings.length + ' unique warnings. ' +
    (decision.bootstrap ? 'Baseline awaiting first complete real run.' : `${decision.added.length} new; ${decision.removed.length} removed.`) + '\n' +
    table('Target', { ...Object.fromEntries(targets.map(t => [t, 0])), ...counts(warnings, 'target') }) + table('Lint', counts(warnings, 'code')) + table('File', counts(warnings, 'file')) +
    '\n### Diagnostics\n\n' + warnings.map(w => `#### ${escape(w.target)}: ${escape(w.file)}:${w.line} (${escape(w.code)})\n\nCrate: ${escape(w.crate)}\n\n\`\`\`text\n${w.rendered.replaceAll('```', "'''")}\n\`\`\`\n`).join('\n');
}
export async function capture(program, args, log) {
  let output;
  try {
    fs.mkdirSync(path.dirname(log), { recursive: true });
    output = fs.createWriteStream(log, { flags: 'a' });
    output.on('error', () => { output = undefined; });
  } catch { output = undefined; }
  const child = spawn(program, args, { stdio: ['inherit', 'pipe', 'pipe'] });
  for (const [source, destination] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
    source.on('data', chunk => { destination.write(chunk); output?.write(chunk); });
  }
  const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('close', code => resolve(code ?? 1)); });
  if (output) await new Promise(resolve => { output.on('error', resolve); output.end(resolve); });
  return code;
}
function options(argv) {
  const result = {};
  for (let i = 0; i < argv.length; i += 2) result[argv[i].replace(/^--/, '')] = argv[i + 1];
  return result;
}
async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === 'run') {
    const split = args.indexOf('--');
    const opts = options(args.slice(0, split));
    process.exitCode = await capture(args[split + 1], args.slice(split + 2), opts.log);
    return;
  }
  const opts = options(args);
  if (mode === 'collect') {
    const warnings = parseWarnings(fs.existsSync(opts.input) ? fs.readFileSync(opts.input, 'utf8') : '', opts);
    fs.mkdirSync(path.dirname(opts.output), { recursive: true });
    fs.writeFileSync(opts.output, JSON.stringify({ version: 1, platform: opts.platform, target: opts.target, complete: opts.complete === 'true' && fs.existsSync(opts.input), warnings }, null, 2));
  } else throw new Error('Use run or collect');
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) await main();

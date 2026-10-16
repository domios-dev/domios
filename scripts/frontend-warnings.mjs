import { deduplicate, normalizeFile, ratchet, stripAnsi } from './collect-rust-warnings.mjs';

export function parseFrontendWarnings(input, { platform, root = process.cwd() } = {}) {
  const lines = stripAnsi(input).replaceAll('\\', '/').replaceAll(root.replaceAll('\\', '/'), '.').split(/\r?\n/);
  const records = [];
  for (let i = 0; i < lines.length; i++) {
    const header = lines[i].trim();
    if (!/^(?:\[[^\]]+\]\s*)?warning\b/i.test(header) && !header.startsWith('(!)')) continue;
    const code = header.match(/^\[([A-Z_]+)\]/)?.[1] ??
      (header.startsWith('(!) Some chunks') ? 'CHUNK_SIZE_LIMIT' : 'VITE_WARNING');
    const message = header.replace(/^\[[A-Z_]+\]\s*Warning:\s*/, '').replace(/^\(!\)\s*/, '');
    const file = header.match(/Warning:\s+(.+?) is dynamically imported/)?.[1] ?? '<frontend>';
    const details = [header];
    while (i + 1 < lines.length && lines[i + 1].trim() && !/^(?:\[[^\]]+\]\s*)?warning\b/i.test(lines[i + 1]) && !lines[i + 1].startsWith('[') && !lines[i + 1].startsWith('(!)')) {
      details.push(lines[++i]);
    }
    records.push({ platform, target: 'frontend', crate: 'vite', file: normalizeFile(file, root), line: 0,
      code, message, rendered: details.join('\n') });
  }
  return deduplicate(records);
}

export function frontendWarningCollection(input, { platform, root, complete }, baseline) {
  const warnings = parseFrontendWarnings(input, { platform, root });
  return { version: 1, platform, target: 'frontend', complete, warnings,
    decision: ratchet(warnings, baseline, ['frontend']) };
}

import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const sourceRoot = path.resolve(process.argv[2] ?? fileURLToPath(new URL('../src-tauri', import.meta.url)));
const core = 'crates/codeterm-service-core/src/service/';
const contentionFixtures = [
  { file: 'crates/codeterm-pty/src/dispatch/tests.rs', binding: 'writer', reason: 'contention fixture' },
  { file: `${core}command_port_impl.rs`, binding: 'lifecycle_owner', reason: 'contention fixture' },
  { file: `${core}status_reconciliation.rs`, binding: '_lifecycle_owner', reason: 'contention fixture' },
  { file: `${core}status_reconciliation.rs`, binding: 'lifecycle_owner', reason: 'contention fixture' },
];
const guard = /\blet\s+([a-z_][a-z_0-9]*)\s*=\s*(?:[a-zA-Z_][a-zA-Z_0-9]*\s*\.\s*)*mux\s*\.\s*write\s*\(\s*\)\s*;/g;
const readMethods = 'snapshot|get_tab|tab_id_for_pty|resolve_tab_id|get_focused_tab_id|other_session_ids|tab_ids_with_session|list_workspaces|to_session_snapshot';
const chainedRead = new RegExp(`\\.\\s*(?:mux|mux_state|state)\\s*\\.\\s*write\\s*\\(\\s*\\)\\s*\\.\\s*(?:${readMethods})\\b`, 'g');
const failures = [];
const usedFixtures = new Set();

function scan(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (['target', 'node_modules', 'gen', 'vendor', 'pty-fork'].includes(entry.name)) continue;
    const target = path.join(directory, entry.name);
    if (entry.isDirectory()) { scan(target); continue; }
    if (!entry.isFile() || !entry.name.endsWith('.rs')) continue;
    const file = path.relative(sourceRoot, target).split(path.sep).join('/');
    const source = readFileSync(target, 'utf8');
    for (const match of source.matchAll(guard)) {
      const fixture = contentionFixtures.find(item => item.file === file && item.binding === match[1]);
      const testOnly = /(?:^|\/)tests(?:\/|\.rs$)/.test(file)
        || /#\[cfg\(test\)\]/.test(source.slice(0, match.index));
      if (fixture && testOnly && !usedFixtures.has(fixture)) {
        usedFixtures.add(fixture);
        continue;
      }
      fail(file, source, match);
    }
    if (file.startsWith('crates/codeterm-service-core/src/')) {
      for (const match of source.matchAll(chainedRead)) fail(file, source, match);
    }
  }
}

function fail(file, source, match) {
  const line = source.slice(0, match.index).split('\n').length;
  failures.push(`${file}:${line}: ${match[0].replace(/\s+/g, ' ')}`);
}

scan(sourceRoot);
if (failures.length) {
  console.error('mux .write() used for read-only access — use .read():\n' + failures.join('\n'));
  process.exitCode = 1;
} else {
  console.log('check-mux-write: no .write() used for read-only mux access');
}

import { resolve } from 'node:path';
import { runDc } from './command.mjs';
import { writeEvidence } from './evidence.mjs';
const [peer, root] = process.argv.slice(2);
if (!peer || !root || !/^[a-f0-9-]{6,}$/.test(peer)) throw new Error('Exact windows identity and run directory required');
const lib = '/Users/dev/Developer/codeterm-worktrees/pl-fni7jw-task-1yh44r-f2ec7f4f/evals/_lib/lib.sh';
for (const [name, args] of [
  ['local-mesh', ['mesh', 'status', '--json']],
  ['remote-mesh', ['mesh', 'status', '--mesh', peer, '--json']],
  ['local-peers', ['mesh', 'peers', '--json']],
  ['remote-peers', ['mesh', 'peers', '--mesh', peer, '--json']],
  ['local-tabs', ['tab', 'list', '--json']],
  ['remote-tabs', ['tab', 'list', '--mesh', peer, '--json']],
  ['local-providers', ['agent', 'providers', '--json']],
  ['remote-providers', ['agent', 'providers', '--mesh', peer, '--json']],
]) {
  let result;
  try { result = await runDc(lib, args); }
  catch (error) { result = { code: -1, error: error.message, stdout: error.stdout, stderr: error.stderr, args }; }
  await writeEvidence(root, `${name}.capture.json`, JSON.stringify(result, null, 2));
  if (result.code !== 0) throw new Error(`Capture failed; preserved ${name}.capture.json`);
}

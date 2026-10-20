import { execFile } from 'node:child_process';
import { writeFileSync } from 'node:fs';

const args = process.argv.slice(2);
const opt = name => args[args.indexOf(name) + 1];
for (const name of ['--bash', '--owner', '--origin', '--out']) {
  if (!args.includes(name)) throw Error(`Missing ${name}`);
}
if (new URL(process.env.CODETERM_API_URL || '').origin !== 'http://127.0.0.1:7686') {
  throw Error('DEV wrapper required');
}
const origin = new URL(opt('--origin'));
if (origin.protocol !== 'http:' || origin.hostname !== '127.0.0.1' ||
    ['7685', '7686', ''].includes(origin.port) || origin.search || origin.pathname !== '/') {
  throw Error('Owned fixture origin required');
}
const owner = opt('--owner');
if (!/^[A-Za-z0-9_-]+$/.test(owner)) throw Error('Invalid owner ID');
const rows = [];
function command(verbArgs) {
  return new Promise(resolve => {
    const start = Date.now();
    const script = 'unset CODETERM_CLI_PATH; source evals/_lib/lib.sh; dc --as-tab "$1" plugin browser "${@:2}"';
    execFile(opt('--bash'), ['-lc', script, 'qa', owner, ...verbArgs], {
      encoding: 'utf8', timeout: 20000,
    }, (error, stdout, stderr) => {
      const row = {
        command: ['dc', '--as-tab', owner, 'plugin', 'browser', ...verbArgs],
        exit: error?.code ?? 0, elapsed_ms: Date.now() - start, stdout, stderr,
      };
      rows.push(row);
      resolve(row);
    });
  });
}
const signal = AbortSignal.timeout(25000);
let failure;
try {
  const gate = fetch(new URL('/gate', origin), { signal });
  const action = command(['click', 'link', 'Held navigation']);
  await gate;
  const result = await action;
  rows.push({ observation: 'response still withheld at click completion', returned_success: result.exit === 0 });
} catch (error) {
  failure = error.name;
  rows.push({ failure });
} finally {
  await fetch(new URL('/release', origin), { signal: AbortSignal.timeout(5000) });
}
if (!failure) {
  await command(['wait', '--text', 'Held destination complete']);
  await command(['snapshot']);
}
writeFileSync(opt('--out'), JSON.stringify(rows, null, 2), { flag: 'wx' });
console.log(JSON.stringify(rows));
if (failure || rows.some(row => row.exit !== undefined && row.exit !== 0)) process.exitCode = 1;

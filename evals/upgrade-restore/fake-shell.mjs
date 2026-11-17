import {appendFileSync} from 'node:fs';

const [trace, tab] = process.argv.slice(2);
if (!trace || !tab || process.env.CODETERM_TAB_ID !== tab) throw new Error('Fixture shell tab identity mismatch');
if (!process.stdin.isTTY || !process.stdout.isTTY) throw new Error('Fixture shell requires a live PTY');
appendFileSync(trace, JSON.stringify({tab_id: tab, pid: process.pid, cwd: process.cwd(),
  nonce: process.env.CODETERM_SESSION_BINDING_NONCE}) + '\n');
process.stdin.resume();
process.stdout.write('\x1b]133;A\x07fixture> ');

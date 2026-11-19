import {appendFileSync, existsSync} from 'node:fs';
import {join} from 'node:path';

const trace = process.argv[2];
const sessionIndex = process.argv.indexOf('--resume');
const sessionId = sessionIndex >= 0 ? process.argv[sessionIndex + 1] : null;
const config = process.env.CLAUDE_CONFIG_DIR;
if (!trace || !sessionId || !config) throw new Error('fake Claude requires a resumed session and isolated config directory');
const project = process.cwd().replace(/[^A-Za-z0-9]/g, '-');
const sessionFile = join(config, 'projects', project, `${sessionId}.jsonl`);
if (!existsSync(sessionFile)) throw new Error(`fake Claude session backing is missing for ${sessionId}`);
appendFileSync(trace, `${JSON.stringify({session_id: sessionId, cwd: process.cwd(), pid: process.pid})}\n`);
process.stdout.write('Claude Code\n> ');
for await (const _chunk of process.stdin) {}

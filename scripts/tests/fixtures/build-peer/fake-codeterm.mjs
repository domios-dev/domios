// Stands in for the codeterm CLI: records each call and answers from FAKE_PEER_STATUS.
import fs from 'node:fs';

const args = process.argv.slice(2);
const stdin = args.includes('--stdin') ? fs.readFileSync(0, 'utf8') : null;
fs.appendFileSync(process.env.FAKE_CODETERM_LOG, `${JSON.stringify({ args, stdin })}\n`);
const statuses = JSON.parse(process.env.FAKE_PEER_STATUS ?? '{}');
const mesh = args.includes('--mesh') ? args[args.indexOf('--mesh') + 1] : null;
const out = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);

if (args[0] === 'mesh' && args[1] === 'status') {
  out({ instance_id: 'a35724e2-c7e3-486b-95d1-5edaa7f8d749', instance_name: 'BLACKSTAR' });
} else if (args[0] === 'tab' && args[1] === 'list') {
  const status = statuses[mesh];
  if (status === 'unreachable') {
    process.stderr.write('peer_unroutable\n');
    process.exit(1);
  }
  out({ tabs: status === 'not_setup' ? [] : [{ id: `status-${mesh}`, title: 'Buildpeer' }] });
} else if (args[0] === 'tab' && args[1] === 'exec') {
  const status = statuses[mesh];
  if (status === 'exec_timeout') out({ data: { completion: 'timeout', output: '' }, ok: false });
  else out({ completion: 'command_finished', exit_code: 0, output: `MSVC env loaded\nPEER_STATUS ${JSON.stringify(status)}\n` });
} else if (args[0] === 'tab' && args[1] === 'new') {
  out({ tab_id: 'remote01', budget_notice: null });
} else if (args[0] === 'send') {
  process.stdout.write('OK (sent)\n');
} else {
  process.stderr.write(`fake codeterm: unhandled ${args.join(' ')}\n`);
  process.exit(2);
}

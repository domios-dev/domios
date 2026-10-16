import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { parse } from 'yaml';

export function gateCommand(args) {
  return './build.sh ' + args.map(arg => /^[a-zA-Z0-9_./:=+-]+$/.test(arg)
    ? arg : `'${arg.replaceAll("'", "'\\''")}'`).join(' ');
}

export function authorizedRustGate({ env, current, canonical, plan, tasks, args }) {
  if (!current || !canonical || !env.CODETERM_TAB_ID || plan?.id !== env.CODETERM_PLAN_ID) return false;
  const owner = plan.owner?.tab_id;
  if (current === canonical && owner && !env.CODETERM_TASK_ID && !env.CODETERM_ATTEMPT_ID) {
    if (env.CODETERM_ROLE === 'Orchestrator' && env.CODETERM_TAB_ID === owner) return true;
    if (!env.CODETERM_ROLE && env.CODETERM_PARENT_TAB_ID === owner) return true;
  }
  if (env.CODETERM_ROLE !== 'Worker' || !env.CODETERM_ATTEMPT_ID || !env.CODETERM_TASK_ID ||
      !env.CODETERM_SESSION_BINDING_NONCE) return false;
  const task = tasks.find(task => task.id === env.CODETERM_TASK_ID);
  if (!task || task.assigned_tab_id !== env.CODETERM_TAB_ID) return false;
  const attempt = task.execution_attempts?.find(attempt => attempt.id === env.CODETERM_ATTEMPT_ID);
  if (!attempt || attempt.executor_tab_id !== env.CODETERM_TAB_ID || attempt.closed_at != null ||
      attempt.retired_at != null || attempt.session_binding_nonce !== env.CODETERM_SESSION_BINDING_NONCE) return false;
  const epoch = task.interruption?.owner_epoch ?? 0;
  if (epoch && attempt.interruption?.owner_epoch !== epoch) return false;
  return Array.isArray(attempt.authorized_gates) && attempt.authorized_gates.includes(gateCommand(args));
}

export function readGateProof(env, read = fs.readFileSync, list = fs.readdirSync) {
  if (!/^pl_[a-zA-Z0-9]+$/.test(env.CODETERM_PLAN_ID ?? '') || !path.isAbsolute(env.CODETERM_PAD_DIR ?? '')) return null;
  // The host pad lives at <instance>/runtime/agent-pads/<tab> beside the plan store.
  const root = path.resolve(env.CODETERM_PAD_DIR, '../../..');
  if (path.basename(path.dirname(env.CODETERM_PAD_DIR)) !== 'agent-pads' ||
      path.basename(path.dirname(path.dirname(env.CODETERM_PAD_DIR))) !== 'runtime') return null;
  const directory = path.join(root, 'plans', 'active', env.CODETERM_PLAN_ID);
  const plan = parse(read(path.join(directory, 'plan.yaml'), 'utf8'));
  const tasks = env.CODETERM_TASK_ID ? list(path.join(directory, 'tasks'))
    .filter(file => file.endsWith('.yaml'))
    .map(file => parse(read(path.join(directory, 'tasks', file), 'utf8'))) : [];
  return { plan, tasks };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [current, canonical, ...args] = process.argv.slice(2);
    const proof = readGateProof(process.env);
    process.exitCode = proof && authorizedRustGate({ env: process.env, current, canonical, args, ...proof }) ? 0 : 2;
  } catch { process.exitCode = 2; }
}

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';

const WAIT_FLAGS = new Set(['--wait-build', '--wait']);
const NOTIFY_FLAGS = new Set(['--notify-tab', '-NotifyTab']);

export function gateArgs(args) {
  const kept = [];
  for (let i = 0; i < args.length; i += 1) {
    if (WAIT_FLAGS.has(args[i])) continue;
    if (NOTIFY_FLAGS.has(args[i])) { i += 1; continue; }
    kept.push(args[i]);
  }
  return kept;
}

function grantMatches(grant, args) {
  return grant.replace(/ --wait(-build)?(?= |$)/g, '').replace(/ (--notify-tab|-NotifyTab) \S+/g, '')
    === gateCommand(gateArgs(args));
}

export function gateCommand(args) {
  return './build.sh ' + args.map(arg => /^[a-zA-Z0-9_./:=+-]+$/.test(arg)
    ? arg : `'${arg.replaceAll("'", "'\\''")}'`).join(' ');
}

export function authorizedRustGate({ env, current, canonical, plan, tasks, taskless_worker, args }) {
  if (!current || !canonical || !env.CODETERM_TAB_ID) return false;
  if (current === canonical && !env.CODETERM_TASK_ID && !env.CODETERM_ATTEMPT_ID &&
      tasklessWorkerRecordMatches(env.CODETERM_TAB_ID, taskless_worker) &&
      taskless_worker.taskless_authorized_gates.some(grant => grantMatches(grant, args))) return true;
  if (!plan || !env.CODETERM_PLAN_ID || plan.id !== env.CODETERM_PLAN_ID) return false;
  const owner = plan.owner?.tab_id;
  if (current === canonical && owner && !env.CODETERM_TASK_ID && !env.CODETERM_ATTEMPT_ID) {
    if (env.CODETERM_ROLE === 'Orchestrator' && env.CODETERM_TAB_ID === owner) return true;
    if (!env.CODETERM_ROLE && env.CODETERM_PARENT_TAB_ID === owner) return true;
  }
  if (!env.CODETERM_ATTEMPT_ID || !env.CODETERM_TASK_ID || !env.CODETERM_SESSION_BINDING_NONCE) return false;
  const task = tasks.find(task => task.id === env.CODETERM_TASK_ID);
  if (!task) return false;
  // A Worker's own utility shell carries the Worker as its parent tab.
  const executor = env.CODETERM_ROLE === 'Worker' ? env.CODETERM_TAB_ID
    : (!env.CODETERM_ROLE ? env.CODETERM_PARENT_TAB_ID : undefined);
  if (!executor || task.assigned_tab_id !== executor) return false;
  const attempt = task.execution_attempts?.find(attempt => attempt.id === env.CODETERM_ATTEMPT_ID);
  if (!attempt || attempt.executor_tab_id !== executor || attempt.closed_at != null ||
      attempt.retired_at != null || attempt.session_binding_nonce !== env.CODETERM_SESSION_BINDING_NONCE) return false;
  const epoch = task.interruption?.owner_epoch ?? 0;
  if (epoch && attempt.interruption?.owner_epoch !== epoch) return false;
  return Array.isArray(attempt.authorized_gates) && attempt.authorized_gates.some(grant => grantMatches(grant, args));
}

function tasklessWorkerRecordMatches(tabId, record) {
  if (!record || record.id !== tabId || record.role !== 'Worker' ||
      typeof record.parent_tab_id !== 'string' || record.parent_tab_id.length === 0 || record.task_binding != null ||
      !Array.isArray(record.taskless_authorized_gates)) return false;
  return record.taskless_authorized_gates.length > 0 &&
    record.taskless_authorized_gates.every(grant => typeof grant === 'string' && grant.length > 0);
}

function readTasklessWorkerRecord(env, run) {
  if (!env.CODETERM_TAB_ID) return null;
  const result = JSON.parse(run('codeterm', ['--json', 'tab', 'inspect', '--tab', env.CODETERM_TAB_ID],
    { encoding: 'utf8', env }));
  const rows = result.data ?? result;
  const record = Array.isArray(rows) ? rows.find(tab => tab?.id === env.CODETERM_TAB_ID) : rows;
  return tasklessWorkerRecordMatches(env.CODETERM_TAB_ID, record) ? record : null;
}

export function readGateProof(env, read = fs.readFileSync, list = fs.readdirSync, run = execFileSync, parseYaml = null) {
  if (env.CODETERM_TASK_ID && env.CODETERM_ATTEMPT_ID) return readGateProofFromHost(env, run);
  if (env.CODETERM_ROLE === 'Worker' && !env.CODETERM_TASK_ID && !env.CODETERM_ATTEMPT_ID) {
    const taskless_worker = readTasklessWorkerRecord(env, run);
    if (taskless_worker) return { plan: { id: env.CODETERM_PLAN_ID ?? null }, tasks: [], taskless_worker };
  }
  if (!/^pl_[a-zA-Z0-9]+$/.test(env.CODETERM_PLAN_ID ?? '') || !path.isAbsolute(env.CODETERM_PAD_DIR ?? '')) return null;
  // The host pad lives at <instance>/runtime/agent-pads/<tab> beside the plan store.
  const root = path.resolve(env.CODETERM_PAD_DIR, '../../..');
  if (path.basename(path.dirname(env.CODETERM_PAD_DIR)) !== 'agent-pads' ||
      path.basename(path.dirname(path.dirname(env.CODETERM_PAD_DIR))) !== 'runtime') return null;
  const directory = path.join(root, 'plans', 'active', env.CODETERM_PLAN_ID);
  let planText;
  try { planText = read(path.join(directory, 'plan.yaml'), 'utf8'); } catch { return readGateProofFromHost(env, run); }
  // Lanes without node_modules still get a decision through the host CLI.
  let parse = parseYaml;
  if (!parse) {
    try { ({ parse } = createRequire(import.meta.url)('yaml')); } catch { return readGateProofFromHost(env, run); }
  }
  const plan = parse(planText);
  const tasks = env.CODETERM_TASK_ID ? list(path.join(directory, 'tasks'))
    .filter(file => file.endsWith('.yaml'))
    .map(file => parse(read(path.join(directory, 'tasks', file), 'utf8'))) : [];
  return { plan, tasks };
}

// The running daemon may keep plans outside the YAML store; ask it through the CLI.
function readGateProofFromHost(env, run = execFileSync) {
  if (!env.CODETERM_TASK_ID) return null;
  const shown = JSON.parse(run('codeterm', ['plan', 'task', 'show', env.CODETERM_PLAN_ID, env.CODETERM_TASK_ID, '--json'],
    { encoding: 'utf8', env }));
  const task = shown.data?.task ?? shown.data ?? shown.task ?? shown;
  return { plan: { id: env.CODETERM_PLAN_ID }, tasks: [task] };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [current, canonical, ...args] = process.argv.slice(2);
    const env = process.env;
    if (current && current === canonical && !env.CODETERM_TASK_ID && !env.CODETERM_ATTEMPT_ID &&
        env.CODETERM_ROLE !== 'Worker') {
      process.exitCode = 0;
    } else {
      const proof = readGateProof(env);
      process.exitCode = proof && authorizedRustGate({ env, current, canonical, args, ...proof }) ? 0 : 2;
    }
  } catch { process.exitCode = 2; }
}

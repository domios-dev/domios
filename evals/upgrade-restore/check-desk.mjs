import {readFileSync} from 'node:fs';

// Mirrors the required fields and typed child locations of SessionSnapshot's serde schema.
const fields = names => names.split(' ');
const object = (required, children = {}, strict = false) => ({required: required ? fields(required) : [], children, strict});

const launch = object('', {
  account_notices: [object('id provider session at_ms content')],
  execution_sandbox: object('intent cwd launch_args writable_roots environment', {runtime_mode: null}, true),
  exited_chat_binding: object('provider_id nonce session_id'),
  context_files: object('', {edit: null, read: null}, true),
  runtime_launch_options: object('provider_id binding_nonce mode additional_directories'),
  worker_restart: object('attempt_id original_brief sender_tab', {
    pending: [object('delivery_id sender_tab provider_id session_id binding_nonce after_offset user_text hops')],
  }),
});
const identity = object('', {
  agent_state: object('state'),
  reasoning_effort: object(''),
  turn_resume: object('session_id start_epoch', {
    sender: object('tab_id label'),
    settled: object('start_epoch outcome'),
  }),
  restorable_launch: launch,
  context_epochs: [object('provider cwd session_id')],
  prior_session_bindings: [object('nonce')],
  task_binding: object('bound_at'),
  managed: object('manager_tab_id manager_name manager_closed'),
});
const leaf = object('tab_id', {
  close_restore: object('stopped exact_session'),
  shell_profile: object('id name executable args'),
  agent_identity: identity,
  neighbor_authority: object('uid friendship_id', {root_tab_id: null}, true),
  ownership: object('controller_title', {team: object('id')}),
  model_fallback: object('requested_model provider_resolved_model reason'),
  model_identity_mismatch: object('requested_model actual_model reason'),
  surface: object('plugin_id surface_id instance_id'),
  watcher: object('charter policy', {policy: object('tick_interval_secs nudge_cooldown_secs max_actions_per_tick')}),
  pending_delivery: object('content_digest channel state', {attempt_provenance: object('attempt_id started_at_ms')}),
});
const split = object('orientation ratio children', {children: [node]});
const snapshot = object('version timestamp workspaces', {
  workspaces: [object('id name tabs', {
    task_keys: object(''),
    tabs: [object('id title split_tree', {split_tree: node})],
  })],
  teams: [object('id created_at workspace_id')],
});

function node(value, path, issues) {
  const schema = value?.type === 'Leaf' ? leaf : value?.type === 'Split' ? split : null;
  if (!schema) {
    issues.push(`${path}.type: missing or invalid SplitNodeSnapshot discriminator`);
    return;
  }
  inspect(value, schema, path, issues);
}

function inspect(value, schema, path, issues) {
  if (typeof schema === 'function') return schema(value, path, issues);
  if (Array.isArray(schema)) {
    if (!Array.isArray(value)) {
      issues.push(`${path}: expected array`);
      return;
    }
    value.forEach((entry, index) => inspect(entry, schema[0], `${path}[${index}]`, issues));
    return;
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    issues.push(`${path}: expected object`);
    return;
  }
  for (const key of schema.required) {
    if (!Object.hasOwn(value, key)) issues.push(`${path}.${key}: missing required field`);
  }
  for (const [key, child] of Object.entries(schema.children)) {
    if (child && Object.hasOwn(value, key) && (value[key] != null || schema.required.includes(key))) {
      inspect(value[key], child, `${path}.${key}`, issues);
    }
  }
  const masked = Object.keys(value).filter(key => /^z+[a-y]?$/.test(key));
  if (masked.length) maskedFields.push({path, key_bytes: masked.map(key => Buffer.byteLength(key))});
  if (schema.strict) {
    const unknown = Object.keys(value).filter(key => !schema.required.includes(key) && !Object.hasOwn(schema.children, key));
    if (unknown.length) issues.push(`${path}: ${unknown.length} unexpected field(s)`);
  }
}

let issues = [];
const maskedFields = [];
try {
  const file = process.argv[2];
  if (!file) throw new Error('usage');
  const document = JSON.parse(readFileSync(file, 'utf8'));
  if (document?.format !== 'upgrade-restore-v2' || document.sanitized !== true) {
    issues.push('A sanitized upgrade-restore-v2 document is required');
  } else {
    inspect(document.snapshot, snapshot, 'snapshot', issues);
  }
} catch {
  issues = ['Usage: node evals/upgrade-restore/check-desk.mjs SANITIZED_CAPTURE (readable JSON required)'];
}
console.log(JSON.stringify({passed: issues.length === 0, issues, masked_fields: maskedFields}, null, 2));
if (issues.length) process.exitCode = 1;

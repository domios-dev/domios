export function leaves(document) {
  const result = [];
  function visit(node) {
    if (node?.type === 'Leaf') result.push(node);
    else if (node?.type === 'Split') node.children.forEach(visit);
    else throw new Error('Malformed snapshot split tree');
  }
  for (const workspace of document.snapshot.workspaces) {
    for (const tab of workspace.tabs) visit(tab.split_tree);
  }
  const ids = result.map(tab => tab.tab_id);
  if (new Set(ids).size !== ids.length) throw new Error('Duplicate snapshot tab id');
  return result;
}

export function validateCapture(document) {
  if (document.format !== 'upgrade-restore-v1' || document.sanitized !== true) {
    throw new Error('A sanitized upgrade-restore-v1 document is required');
  }
  const tabs = leaves(document);
  if (!Array.isArray(document.plans) || !Array.isArray(document.live_tab_metadata)) {
    throw new Error('Plan store and live hidden-tab metadata are required');
  }
  if (!tabs.length) throw new Error('Empty desk cannot prove restore');
  const measurements = new Map(document.measurements.map(row => [row.tab_id, row]));
  const histories = new Map(document.histories.map(row => [row.tab_id, row]));
  for (const tab of tabs) {
    const measure = measurements.get(tab.tab_id);
    const history = histories.get(tab.tab_id);
    if (!measure || !history) throw new Error(`tab=${tab.tab_id}: missing size/history evidence`);
    const identity = tab.agent_identity ?? {};
    const sizes = {
      leaf_bytes: Buffer.byteLength(JSON.stringify(tab)),
      prompt_bytes: Buffer.byteLength(identity.agent_runtime_config?.systemPrompt ?? ''),
      identity_bytes: Buffer.byteLength(identity.restorable_launch?.identity_instructions ?? ''),
      history_bytes: Buffer.byteLength(JSON.stringify(history.messages)),
      history_pages: Math.ceil(history.messages.length / history.page_size),
      ...historyShape(history.messages),
    };
    if (history.page_size !== 100) throw new Error(`tab=${tab.tab_id}: unsupported capture page size`);
    for (const [key, value] of Object.entries(sizes)) {
      if (measure[key] !== value) throw new Error(`tab=${tab.tab_id}: ${key} changed (${measure[key]} -> ${value})`);
    }
    for (const content of [identity.agent_runtime_config?.systemPrompt,
      identity.restorable_launch?.identity_instructions, ...history.messages.map(message => message.content)]) {
      if (typeof content === 'string' && !/^[x¢□🟦\\\n\u0000]*$/u.test(content)) {
        throw new Error(`tab=${tab.tab_id}: unmasked content`);
      }
    }
  }
  return tabs;
}

export function requireBackings(document) {
  const tabs = validateCapture(document);
  const ids = new Set(tabs.map(tab => tab.tab_id));
  for (const live of document.live_tab_metadata) {
    if (!ids.has(live.tab_id)) throw new Error(`tab=${live.tab_id}: hidden/live tab has no persisted replay backing`);
  }
  for (const tab of tabs) {
    if (!tab.agent_type) throw new Error(`tab=${tab.tab_id}: fake shell/surface backing missing`);
    if (tab.agent_type && !(tab.claude_session_id ?? tab.agent_session_id)) {
      throw new Error(`tab=${tab.tab_id}: provider thread identity missing`);
    }
    if (tab.agent_type && tab.agent_type !== 'codex') {
      throw new Error(`tab=${tab.tab_id}: fake provider backing missing for ${tab.agent_type}`);
    }
    const binding = tab.agent_identity?.task_binding;
    if (!binding?.plan_id || !binding?.task_id) continue;
    const plan = document.plans.find(plan => plan.id === binding.plan_id);
    const task = plan?.tasks.find(task => task.id === binding.task_id);
    const attempts = task?.execution_attempts.filter(attempt => attempt.executor_tab_id === tab.tab_id
      && !attempt.closed_at && !attempt.retired_at
      && (!binding.attempt_id || binding.attempt_id === attempt.id)) ?? [];
    if (attempts.length !== 1) throw new Error(`tab=${tab.tab_id}: binding incomplete: ${attempts.length} matching live attempts`);
  }
  return tabs;
}

export function failureReason(error) {
  if (error.killed) return 'bounded replay deadline elapsed';
  try {
    const response = JSON.parse(error.stdout);
    if (response.error?.message) return response.error.message;
  } catch {}
  return error.stderr?.trim() || error.message;
}

export function requireRestart(stage, boot, digest) {
  if (!stage?.boot_id || !boot || stage.boot_id === boot) {
    throw new Error('Target daemon has not restarted since fixture import');
  }
  if (stage.fixture_sha256 !== digest) throw new Error('Fixture differs from the staged document');
}

export function verdict(expected, actual) {
  const id = expected.tab_id;
  if (!actual) return {tab_id: id, ready: false, reason: 'tab missing after restore'};
  for (const key of ['agent_type', 'role', 'role_profile', 'team_id', 'parent_tab_id']) {
    if ((expected[key] ?? null) !== (actual[key] ?? null)) {
      return {tab_id: id, ready: false, reason: `${key} changed after restore`};
    }
  }
  const expectedSession = expected.claude_session_id ?? expected.agent_session_id;
  if (expectedSession && expectedSession !== actual.agent_session_id) {
    return {tab_id: id, ready: false, reason: 'session binding changed after restore'};
  }
  const binding = expected.agent_identity?.task_binding;
  if (binding?.plan_id && binding?.task_id) {
    const current = actual.task_binding;
    if (current?.plan_id !== binding.plan_id || current?.task_id !== binding.task_id || !current?.attempt_id) {
      return {tab_id: id, ready: false, reason: 'binding incomplete: plan/task/attempt required'};
    }
    if (binding.attempt_id && current.attempt_id !== binding.attempt_id) {
      return {tab_id: id, ready: false, reason: 'attempt binding changed after restore'};
    }
  }
  if (!expected.agent_type) return {tab_id: id, ready: false, reason: 'shell/surface readiness backing missing'};
  const runtime = actual.agent_runtime_state;
  if (runtime?.state !== 'loaded' || !(runtime.epoch > 0)) {
    return {tab_id: id, ready: false, reason: runtime?.reason ?? `runtime ${runtime?.state ?? 'missing'}`};
  }
  if (!actual.session_binding_nonce || actual.session_binding_nonce !== actual.reported_session_binding_nonce) {
    return {tab_id: id, ready: false, reason: 'binding incomplete: runtime nonce unconfirmed'};
  }
  return {tab_id: id, ready: true, reason: 'runtime loaded with authoritative binding'};
}

export function measurements(snapshot, histories) {
  const historyByTab = new Map(histories.map(row => [row.tab_id, row]));
  return leaves({snapshot}).map(tab => {
    const history = historyByTab.get(tab.tab_id);
    return {
      tab_id: tab.tab_id,
      leaf_bytes: Buffer.byteLength(JSON.stringify(tab)),
      prompt_bytes: Buffer.byteLength(tab.agent_identity?.agent_runtime_config?.systemPrompt ?? ''),
      identity_bytes: Buffer.byteLength(tab.agent_identity?.restorable_launch?.identity_instructions ?? ''),
      history_bytes: Buffer.byteLength(JSON.stringify(history.messages)),
      history_pages: Math.ceil(history.messages.length / 100),
      ...historyShape(history.messages),
    };
  });
}

function historyShape(messages) {
  const images = messages.flatMap(message => message.attachments ?? []);
  return {
    history_items: messages.length,
    normalized_turn_groups: new Set(messages.map(message => message.toolFlowId ?? 'fixture-turn')).size,
    image_items: images.length + messages.filter(message => message.imageData != null).length,
    image_bytes: images.reduce((sum, image) => sum + Buffer.byteLength(image.data ?? ''), 0)
      + messages.reduce((sum, message) => sum + Buffer.byteLength(message.imageData ?? ''), 0),
  };
}

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

export const FORMAT = 'upgrade-restore-v2';
const PAGE_SIZE = 100;
const SHAPE_KEYS = new Set(['kind', 'group', 'bytes', 'result_bytes', 'input_bytes', 'image_bytes', 'image_data_bytes']);
const HISTORY_KEYS = new Set(['tab_id', 'page_size', 'shape', 'transcript']);
const EMPTY_INPUT_BYTES = Buffer.byteLength(JSON.stringify({fixture: ''}));
const size = value => Number.isSafeInteger(value) && value >= 0;

function validateShape(history) {
  const id = history.tab_id;
  if (Object.keys(history).some(key => !HISTORY_KEYS.has(key)) || !Array.isArray(history.shape)) {
    throw new Error(`tab=${id}: history must be a shape descriptor without transcript content`);
  }
  let groups = 0;
  for (const [index, row] of history.shape.entries()) {
    const keys = Object.keys(row ?? {});
    if (keys.some(key => !SHAPE_KEYS.has(key)) || typeof row.kind !== 'string' || !/^[a-z_]+$/.test(row.kind)
      || !size(row.group) || row.group > groups || !size(row.bytes)
      || ['result_bytes', 'input_bytes', 'image_data_bytes'].some(key => key in row && !size(row[key]))
      || ('image_bytes' in row && !(Array.isArray(row.image_bytes) && row.image_bytes.length && row.image_bytes.every(size)))) {
      throw new Error(`tab=${id}: malformed history shape row ${index}`);
    }
    if (row.group === groups) groups++;
  }
}

// Replay text is derived only from the captured shape, so peer and verifier synthesize identical rows.
export function synthesizeMessage(row, index) {
  const id = `message-${index}`;
  return {id, seq: index + 1, type: row.kind, content: 'x'.repeat(row.bytes), toolFlowId: `turn-${row.group}`,
    ...('result_bytes' in row ? {toolResult: 'x'.repeat(row.result_bytes)} : {}),
    ...('input_bytes' in row ? {toolInput: {fixture: 'x'.repeat(Math.max(0, row.input_bytes - EMPTY_INPUT_BYTES))}} : {}),
    ...(row.image_bytes ? {attachments: row.image_bytes.map((bytes, image) => ({id: `${id}-image-${image + 1}`,
      mimeType: 'image/png', data: bytes ? 'x'.repeat(bytes) : 'eA=='}))} : {}),
  };
}

export function synthesizeHistory(shape) {
  return shape.map(synthesizeMessage);
}

export function validateCapture(document) {
  if (document.format !== FORMAT || document.sanitized !== true) {
    throw new Error(`A sanitized ${FORMAT} document is required`);
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
    if (history.page_size !== PAGE_SIZE) throw new Error(`tab=${tab.tab_id}: unsupported capture page size`);
    validateShape(history);
    const identity = tab.agent_identity ?? {};
    const sizes = {...leafSizes(tab), ...shapeSizes(history.shape)};
    for (const [key, value] of Object.entries(sizes)) {
      if (measure[key] !== value) throw new Error(`tab=${tab.tab_id}: ${key} changed (${measure[key]} -> ${value})`);
    }
    if (!size(measure.history_bytes) || measure.history_bytes < shapePayloadBytes(history.shape)) {
      throw new Error(`tab=${tab.tab_id}: history_bytes is smaller than its shape payload`);
    }
    for (const content of [identity.agent_runtime_config?.systemPrompt,
      identity.restorable_launch?.identity_instructions]) {
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
    const transcript = document.histories.find(row => row.tab_id === tab.tab_id)?.transcript;
    if (transcript?.status === 'unavailable') {
      throw new Error(`tab=${tab.tab_id}: transcript unavailable (backing=${transcript.backing_kind}, provider=${transcript.provider ?? 'none'})`);
    }
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

export function historyMatches(expected, actual) {
  if (!actual || actual.id !== expected.id || actual.type !== expected.type) return false;
  let content = expected.content ?? '';
  const images = expected.type === 'user' ? expected.attachments ?? [] : [];
  for (const image of images) {
    const url = `data:${image.mimeType ?? 'image/png'};base64,${image.data ?? 'eA=='}`;
    content += `\n[image omitted: ${Buffer.byteLength(url)} bytes]`;
  }
  if ((actual.content ?? '') !== content) return false;
  if (!images.length) return true;
  return actual.attachments?.length === images.length && images.every((_, index) => {
    const attachment = actual.attachments[index];
    return attachment.id === `${expected.id}-image-${index + 1}` && attachment.mimeType === 'image/png'
      && attachment.data == null && attachment.url == null;
  });
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
  // A loaded runtime still pages history; chat holds only the pages read so far until it completes.
  if (runtime.history === 'failed') {
    return {tab_id: id, ready: false, terminal: true,
      reason: `history failed after ${runtime.historyPages ?? 0} pages: ${runtime.historyError ?? 'unknown'}`};
  }
  if (runtime.history !== 'complete') {
    return {tab_id: id, ready: false, reason: `history ${runtime.history ?? 'missing'} after ${runtime.historyPages ?? 0} pages`};
  }
  return {tab_id: id, ready: true, reason: 'runtime loaded with authoritative binding'};
}

// Captured history_bytes is real transcript volume; without a prior capture it is the synthesized volume.
export function measurements(snapshot, histories, captured = []) {
  const historyByTab = new Map(histories.map(row => [row.tab_id, row]));
  const capturedByTab = new Map(captured.map(row => [row.tab_id, row]));
  return leaves({snapshot}).map(tab => {
    const shape = historyByTab.get(tab.tab_id).shape;
    return {
      tab_id: tab.tab_id,
      ...leafSizes(tab),
      history_bytes: capturedByTab.get(tab.tab_id)?.history_bytes
        ?? Buffer.byteLength(JSON.stringify(synthesizeHistory(shape))),
      ...shapeSizes(shape),
    };
  });
}

function leafSizes(tab) {
  return {
    leaf_bytes: Buffer.byteLength(JSON.stringify(tab)),
    prompt_bytes: Buffer.byteLength(tab.agent_identity?.agent_runtime_config?.systemPrompt ?? ''),
    identity_bytes: Buffer.byteLength(tab.agent_identity?.restorable_launch?.identity_instructions ?? ''),
  };
}

function shapeSizes(shape) {
  const images = shape.flatMap(row => row.image_bytes ?? []);
  const standalone = shape.filter(row => row.image_data_bytes != null);
  return {
    history_pages: Math.ceil(shape.length / PAGE_SIZE),
    history_items: shape.length,
    normalized_turn_groups: new Set(shape.map(row => row.group)).size,
    image_items: images.length + standalone.length,
    image_bytes: images.reduce((sum, bytes) => sum + bytes, 0)
      + standalone.reduce((sum, row) => sum + row.image_data_bytes, 0),
  };
}

function shapePayloadBytes(shape) {
  return shape.reduce((sum, row) => sum + row.bytes + (row.result_bytes ?? 0) + (row.input_bytes ?? 0)
    + (row.image_bytes ?? []).reduce((total, bytes) => total + bytes, 0) + (row.image_data_bytes ?? 0), 0);
}

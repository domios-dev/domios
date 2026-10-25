export function githubContext(overrides = {}) {
  const workflow_run = { conclusion: '', event: '', path: '', head_branch: '', head_sha: '',
    head_repository: { full_name: '' }, actor: { login: '' }, triggering_actor: { login: '' },
    ...overrides.event?.workflow_run };
  return { repository: '', repository_owner: '', actor: '', triggering_actor: '', workflow: '',
    event_name: '', ref: '', ref_name: '', head_ref: '', base_ref: '', sha: '',
    ...overrides, event: { pull_request: {}, ...overrides.event, workflow_run } };
}

export function evaluateCondition(expression, { needs = {}, inputs = {}, vars = {}, github = {},
  failed = false, cancelled = false, steps = {}, matrix = {}, runner = {}, secrets = {}, env = {} } = {}) {
  if (typeof expression !== 'string') return expression;
  const source = expression.replace(/^\$\{\{\s*|\s*\}\}$/g, '')
    .replaceAll(/(needs|steps)\.([\w-]+)/g, (_, scope, id) => scope + '[' + JSON.stringify(id) + ']');
  const startsWith = (value, prefix) => String(value ?? '').toLowerCase().startsWith(String(prefix).toLowerCase());
  const format = (pattern, ...values) => pattern.replace(/\{(\d+)\}/g, (_, index) => String(values[Number(index)]));
  const variables = new Proxy(vars, { get: (target, key) => target[key] ?? '' });
  return Function('needs', 'steps', 'inputs', 'vars', 'github', 'matrix', 'runner', 'secrets', 'env',
    'startsWith', 'format', 'failure', 'cancelled', 'always', 'fromJSON', 'toJSON', 'contains', 'return (' + source + ')')(
    needs, steps, inputs, variables, githubContext(github), matrix, runner, secrets, env,
    startsWith, format, () => failed, () => cancelled, () => true, JSON.parse, JSON.stringify, (haystack, needle) => haystack.includes(needle));
}

export function condition(expression, context = {}) {
  return Boolean(evaluateCondition(expression, context));
}

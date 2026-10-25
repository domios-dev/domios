import fs from 'node:fs';

const root = new URL('../', import.meta.url);
const catalogue = JSON.parse(fs.readFileSync(new URL('tests/catalogue.json', root))).runners;
const packageScripts = JSON.parse(fs.readFileSync(new URL('package.json', root))).scripts;

// Indirect executables belong to the entrypoint that invokes them, never a workflow job name.
const entryTools = {
  'node-script-tests': ['git', 'bash', 'cargo', 'nextest'],
  'node-script-tests-serial': ['git', 'bash'],
  'python-script-tests': ['python', 'python:yaml'],
};
const scriptTools = {
  'ci-python-env.mjs': ['python'],
  'ci-artifact.sh': ['node', 'bash', 'curl'],
  'ci-qualification.mjs': [],
  'ci-git-history.mjs': ['git'],
  'secret-scan.sh': ['curl', 'tar'],
  'export-chat-types.mjs': ['cargo'],
  'ci-workspace-cleanup.mjs': ['cargo'],
};
const buildTools = {
  'ci-clippy': ['cargo'],
  'ci-cli-tests': ['cargo'],
  'ci-tests': ['cargo', 'nextest', 'python'],
  'cache-release': [],
  'cache-stats': [],
};
export function commandTools(command, seen = new Set()) {
  const required = new Set();
  const add = tools => tools.forEach(tool => required.add(tool));
  command = command.replace(/node\s+(?:\S*\/)?scripts\/ci-runner-paths\.mjs[^\n]*/g, '');
  const entries = [...command.matchAll(/fromJSON\(steps\.catalogue\.outputs\.commands\)\['([^']+)'\]|--entry\s+([\w-]+)|(?:ci-catalogue-command|ci-step)\.mjs\s+([\w-]+)/g)];
  for (const match of entries) {
    const id = match[1] || match[2] || match[3];
    if (seen.has('entry:' + id)) continue;
    const entry = catalogue.find(entry => entry.id === id);
    if (!entry) throw new Error('Unknown catalogue entry: ' + id);
    seen.add('entry:' + id);
    add(entryTools[id] || []);
    add(commandTools(entry.exec?.join(' ') || entry.runner, seen));
  }
  for (const [, name] of command.matchAll(/\bpnpm\s+(?:run\s+)?([\w:-]+)/g)) {
    if (!packageScripts[name] || seen.has('pnpm:' + name)) continue;
    seen.add('pnpm:' + name);
    add(commandTools(packageScripts[name], seen));
  }
  const executableText = command.replace(/#.*$/gm, '');
  for (const tool of ['node', 'git', 'bash', 'cargo', 'gh', 'pnpm', 'jq', 'curl', 'tar']) {
    if (new RegExp('(?:^|[\\s;&|])' + tool + '(?=[\\s;&|]|$)', 'm').test(executableText)) required.add(tool);
  }
  if (/\bpython3?\s/.test(executableText)) required.add('python');
  if (/\bcargo(?:-nextest|\s+nextest)\b/.test(executableText)) add(['cargo', 'nextest']);
  for (const [script, tools] of Object.entries(scriptTools)) if (command.includes('scripts/' + script)) add(tools);
  for (const [, verb] of command.matchAll(/build\.(?:sh|ps1)\s+([\w-]+)/g)) {
    if (!(verb in buildTools)) throw new Error('Declare tool dependencies for build verb: ' + verb);
    add(['node', 'bash', ...buildTools[verb]]);
  }
  return [...required].sort();
}
export function jobTools(job) {
  const commands = (job.steps || []).map(step => step.run || '').filter(command => command && !command.includes('ci-runner-paths.mjs'));
  const aggregate = commands.length && commands.every(command => command === 'node scripts/ci-result.mjs');
  const tools = new Set(aggregate ? ['node'] : ['node', 'git', 'bash']);
  for (const step of job.steps || []) {
    for (const tool of commandTools(step.run || '')) tools.add(tool);
    for (const value of Object.values(step.env || {})) {
      if (String(value).includes('catalogue.outputs.commands')) for (const tool of commandTools(value)) tools.add(tool);
    }
    if (step.uses === './.github/actions/rust-warnings') for (const tool of scriptTools['ci-artifact.sh']) tools.add(tool);
  }
  return [...tools].sort();
}

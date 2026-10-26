import fs from 'node:fs';
import {inferRunnerProfile,runnerProfile} from './ci-runner-profile.mjs';
import {reproLine} from './ci-repro.mjs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function runnerToolPaths(platform, env, hasFile, children = () => []) {
  const windows = platform === 'win32';
  const paths = windows ? path.win32 : path.posix;
  const directories = [];
  if (windows) {
    const gitRoots = [
      env.ProgramFiles && paths.join(env.ProgramFiles, 'Git'),
      env['ProgramFiles(x86)'] && paths.join(env['ProgramFiles(x86)'], 'Git'),
      env.LOCALAPPDATA && paths.join(env.LOCALAPPDATA, 'Programs', 'Git'),
    ].filter(Boolean);
    const inherited = (env.PATH || '').split(';').map((entry) => entry.replace(/^"|"$/g, '')).filter(Boolean);
    const candidates = [
      ...gitRoots.map((root) => paths.join(root, 'bin')),
      ...inherited,
      ...inherited.filter((directory) => /[\\/]cmd$/i.test(directory)).map((directory) => paths.join(directory, '..', 'bin')),
    ];
    const bash = candidates.find((directory) => hasFile(paths.join(directory, 'bash.exe')));
    if (!bash) throw new Error('Git Bash is required by Rust setup and capped tests; restore Git for Windows on the runner');
    directories.push(bash);
    const pythonRoots = [
      env.LOCALAPPDATA && paths.join(env.LOCALAPPDATA, 'Programs', 'Python'),
      env.ProgramFiles,
    ].filter(Boolean);
    const python = [
      env.pythonLocation,
      ...inherited.filter(directory => !/[\\/]WindowsApps(?:[\\/]|$)/i.test(directory)),
      ...pythonRoots.flatMap(root => children(root).filter(name => /^Python\d/i.test(name))
        .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
        .map(name => paths.join(root, name))),
    ].filter(Boolean).find(directory => hasFile(paths.join(directory, 'python.exe')));
    if (python) directories.push(python);
  }
  const home = windows ? env.USERPROFILE : env.HOME;
  const rust = [
    env.CARGO_HOME && paths.join(env.CARGO_HOME, 'bin'),
    windows && home && paths.join(home, 'tools', 'rustup-proxies'),
    home && paths.join(home, '.cargo', 'bin'),
  ].filter(Boolean);
  directories.push(...rust.filter((directory) => hasFile(paths.join(directory, windows ? 'rustup.exe' : 'rustup'))));
  if (directories.some((directory) => /[\r\n]/.test(directory))) throw new Error('Tool path contains a line break');
  return [...new Set(directories)];
}

export function bashInterpreter(platform = process.platform, env = process.env, hasFile = fs.existsSync) {
  if (platform !== 'win32') return 'bash';
  const directories = runnerToolPaths(platform, env, hasFile);
  return path.win32.join(directories[0], 'bash.exe');
}

export const RUNNER_ROLES = {
  policy: ['node', 'git', 'bash', 'python', 'jq', 'curl', 'tar'],
  receipt: ['node', 'git', 'bash', 'curl', 'tar'],
  frontend: ['node', 'git', 'bash', 'pnpm'],
  'cli-contracts': ['node', 'git', 'bash', 'cargo'],
  rust: ['node', 'git', 'bash', 'python', 'cargo', 'nextest'],
  'pr-guards': ['node', 'git', 'bash', 'python', 'cargo', 'pnpm', 'nextest'],
  packaging: ['node', 'git', 'bash', 'python', 'cargo', 'gh', 'pnpm'],
  finalize: ['node', 'git', 'bash', 'python', 'gh', 'pnpm'],
  all: ['node', 'git', 'bash', 'python', 'cargo', 'nextest', 'gh', 'pnpm'],
};
const tools = {
  node: ['node', '--version'], git: ['git', '--version'], bash: ['bash', '--version'],
  cargo: ['cargo', '--version'], nextest: ['cargo-nextest', '--version'],
  gh: ['gh', '--version'], pnpm: ['pnpm', '--version'], jq: ['jq', '--version'],
  curl: ['curl', '--version'], tar: ['tar', '--version'],
};

export function pythonInterpreter(platform, env, hasFile = file => {
  try { return fs.statSync(file).isFile(); } catch { return false; }
}) {
  if (env.CI_PYTHON) return env.CI_PYTHON;
  const paths = platform === 'win32' ? path.win32 : path.posix;
  const directories = (env.PATH || '').split(platform === 'win32' ? ';' : ':');
  const preferred=env.CI_RUNNER_PROFILE?runnerProfile(env.CI_RUNNER_PROFILE).python:null;
  const names=platform==='win32'?['python.exe']:preferred?[preferred,...['python3','python'].filter(name=>name!==preferred)]:['python3','python'];
  for (const name of names) {
    for (const directory of directories.filter(Boolean)) {
      const candidate = paths.join(directory, name);
      if (hasFile(candidate)) return candidate;
    }
  }
  throw new Error('Python interpreter unavailable: restore Python and its venv module using .agents/skills/build-machine/SKILL.md');
}

export function runnerRequirements(role) {
  if (RUNNER_ROLES[role]) return RUNNER_ROLES[role];
  const names = role.split(',');
  if (!names.length || names.some(name => !(name in tools) && name !== 'python' && !/^python:[A-Za-z_][A-Za-z0-9_]*(?:[.][A-Za-z_][A-Za-z0-9_]*)*$/.test(name))) {
    throw new Error('Unknown runner role or tool list: ' + role);
  }
  return names;
}

export function checkRunnerTools(role, { platform = process.platform, env = process.env,
  execute = (command, args, options) => spawnSync(command, args, options) } = {}) {
  const required = runnerRequirements(role);
  const missing = [];
  const diagnostics = [];
  for (const tool of required) {
    const python = env.CI_PYTHON || (platform === 'win32' ? 'python' : 'python3');
    const [command, ...args] = tool.startsWith('python:')
      ? [python, '-c', 'import ' + tool.slice(7)]
      : tool === 'python' ? [python, '--version'] : tools[tool];
    const probeEnv = env;
    // A first launch under arm64 emulation can take seconds; the bound only stops a hung tool.
    const result = execute(command, args, { env: probeEnv, encoding: 'utf8', timeout: 30000,
      windowsHide: true, shell: platform === 'win32' && tool === 'pnpm' });
    if (result.error || result.status !== 0) {
      missing.push(tool);
      const status = result.error?.code || result.signal || ('exit ' + result.status);
      diagnostics.push(tool + ': ' + command + ' ' + args.join(' ') + ' (' + status + ')' +
        (result.stderr ? '\n' + String(result.stderr).trim() : '') +
        (result.stdout ? '\nstdout: ' + String(result.stdout).trim() : ''));
    }
  }
  if (missing.length) throw new Error('Runner preflight (' + role + ') missing or broken: ' +
    missing.join(', ') + '. Install/restore tools using .agents/skills/build-machine/SKILL.md; ' +
    'nextest: cargo install cargo-nextest --locked; Python: https://www.python.org/downloads/; ' +
    'Git Bash: https://gitforwindows.org/; Node/pnpm: https://nodejs.org/ and corepack enable; gh: https://cli.github.com/; ' +
    'Python modules: python -m pip install -r scripts/requirements-ci.txt.\n' + diagnostics.join('\n'));
}

// build.sh resolves tools from Git Bash, which can differ from the cmd lookup above; refuse before compilation.
export const BASH_RESOLVED_TOOLS = ['node', 'git', 'cargo', 'pnpm'];

export function checkBashTools(role, bash, { env = process.env,
  execute = (command, args, options) => spawnSync(command, args, options) } = {}) {
  const tools = runnerRequirements(role).filter(tool => BASH_RESOLVED_TOOLS.includes(tool));
  if (!tools.length) return;
  const options = { env, encoding: 'utf8', timeout: 10000, windowsHide: true };
  const script = 'for tool in ' + tools.join(' ') + '; do command -v "$tool" >/dev/null 2>&1 || echo "missing:$tool"; done';
  const result = execute(bash, ['-c', script], options);
  const missing = String(result.stdout || '').split(/\r?\n/).filter(line => line.startsWith('missing:')).map(line => line.slice(8));
  if (!result.error && result.status === 0 && !missing.length) return;
  const seen = execute(bash, ['-c', 'printf "%s\\n" "$PATH" | tr : "\\n"'], options);
  throw new Error('Git Bash used by build.sh cannot resolve: ' +
    (missing.join(', ') || String(result.error?.code || 'exit ' + result.status)) +
    '\nGit Bash PATH:\n' + String(seen.stdout || '').trim());
}

export function runnerPathContent(directories) {
  return directories.map(directory => directory + '\n').join('');
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
  if (!process.env.GITHUB_PATH) throw new Error('GITHUB_PATH is required for job-local tool setup');
  const hasFile = (file) => {
    try { return fs.statSync(file).isFile(); } catch { return false; }
  };
  const children = directory => {
    try { return fs.readdirSync(directory, { withFileTypes: true }).filter(entry => entry.isDirectory()).map(entry => entry.name); }
    catch { return []; }
  };
  const directories = runnerToolPaths(process.platform, process.env, hasFile, children);
  const home = process.env.HOME || process.env.USERPROFILE;
  const extras = [process.env.PNPM_HOME, home && path.join(home, '.local', 'bin'),
    ...(process.platform === 'darwin' ? ['/opt/homebrew/bin', '/usr/local/bin'] : [])].filter(Boolean);
  directories.push(...extras.filter(directory => fs.existsSync(directory)));
  fs.appendFileSync(process.env.GITHUB_PATH, runnerPathContent(directories));
  const env = { ...process.env, PATH: [...directories, process.env.PATH].join(path.delimiter) };
  if (process.env.GITHUB_ENV && !env.CI_PYTHON) {
    try { fs.appendFileSync(process.env.GITHUB_ENV, 'CI_PYTHON=' + pythonInterpreter(process.platform, env) + '\n'); }
    catch { /* Python is optional for jobs whose declared tools exclude it. */ }
  }
  for (const directory of directories) console.log('Job-local tool directory: ' + directory);
  } catch(error) {
    console.error(error.stack||error.message);
    console.error(reproLine(process.env.CI_STEP||'node-script-tests',{profile:inferRunnerProfile()}));
    process.exitCode=1;
  }

}

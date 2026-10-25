import fs from 'node:fs';
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

export const RUNNER_ROLES = {
  policy: ['node', 'git', 'bash', 'python', 'gh', 'jq', 'curl', 'tar'],
  receipt: ['node', 'git', 'bash', 'gh', 'curl', 'tar'],
  frontend: ['node', 'git', 'bash', 'pnpm'],
  'cli-contracts': ['node', 'git', 'bash', 'cargo'],
  rust: ['node', 'git', 'bash', 'python', 'cargo', 'nextest', 'gh'],
  'pr-guards': ['node', 'git', 'bash', 'python', 'cargo', 'pnpm'],
  packaging: ['node', 'git', 'bash', 'python', 'cargo', 'gh', 'pnpm'],
  all: ['node', 'git', 'bash', 'python', 'cargo', 'nextest', 'gh', 'pnpm'],
};
const tools = {
  node: ['node', '--version'], git: ['git', '--version'], bash: ['bash', '--version'],
  cargo: ['cargo', '--version'], nextest: ['cargo-nextest', '--version'],
  gh: ['gh', '--version'], pnpm: ['pnpm', '--version'], jq: ['jq', '--version'],
  curl: ['curl', '--version'], tar: ['tar', '--version'],
};

export function checkRunnerTools(role, { platform = process.platform, env = process.env,
  execute = (command, args, options) => spawnSync(command, args, options) } = {}) {
  if (!RUNNER_ROLES[role]) throw new Error('Unknown runner role: ' + role);
  const missing = [];
  for (const tool of RUNNER_ROLES[role]) {
    const [command, ...args] = tool === 'python'
      ? [platform === 'win32' ? 'python' : 'python3', '--version'] : tools[tool];
    const probeEnv = tool === 'cargo' && env.RUNNER_ENVIRONMENT === 'github-hosted' ? { ...env, RUSTUP_TOOLCHAIN: 'stable' } : env;
    const result = execute(command, args, { env: probeEnv, encoding: 'utf8', timeout: 2000,
      windowsHide: true, shell: platform === 'win32' && tool === 'pnpm' });
    if (result.error || result.status !== 0) missing.push(tool);
  }
  if (missing.length) throw new Error('Runner preflight (' + role + ') missing or broken: ' +
    missing.join(', ') + '. Install/restore tools using .agents/skills/build-machine/SKILL.md; ' +
    'nextest: cargo install cargo-nextest --locked; Python: https://www.python.org/downloads/; ' +
    'Git Bash: https://gitforwindows.org/; Node/pnpm: https://nodejs.org/ and corepack enable; gh: https://cli.github.com/');
}

export function runnerPathContent(directories) {
  return directories.map(directory => directory + '\n').join('');
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
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
  checkRunnerTools(process.argv[2] || 'rust', { env });
  for (const directory of directories) console.log('Job-local tool directory: ' + directory);
}

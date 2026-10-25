// Caller: CI tests-rust.yml; scripts tests/ci-runner-paths.test.mjs; skill build-machine
import fs from 'node:fs';
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
  fs.appendFileSync(process.env.GITHUB_PATH, directories.map((directory) => `${directory}\n`).join(''));
  if (process.platform === 'win32' && !directories.some(directory => hasFile(path.join(directory, 'python.exe')))) {
    throw new Error('Python is required by Windows native runtime tests; install an interpreter before qualification');
  }
  for (const directory of directories) console.log(`Job-local tool directory: ${directory}`);
}

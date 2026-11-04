import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';

export function bashInterpreter(platform = process.platform, env = process.env, hasFile = fs.existsSync) {
  if (platform !== 'win32') return 'bash';
  const roots = [
    env.ProgramFiles && path.win32.join(env.ProgramFiles, 'Git'),
    env['ProgramFiles(x86)'] && path.win32.join(env['ProgramFiles(x86)'], 'Git'),
    env.LOCALAPPDATA && path.win32.join(env.LOCALAPPDATA, 'Programs', 'Git'),
  ].filter(Boolean);
  const inherited = (env.PATH || '').split(';').map(entry => entry.replace(/^"|"$/g, '')).filter(Boolean);
  const candidates = [
    ...roots.flatMap(root => [path.win32.join(root, 'bin'), path.win32.join(root, 'usr', 'bin')]),
    ...inherited,
    ...inherited.filter(directory => /[\\/]cmd$/i.test(directory)).map(directory => path.win32.join(directory, '..', 'bin')),
    'C:\\msys64\\usr\\bin',
  ];
  const directory = candidates.find(directory => hasFile(path.win32.join(directory, 'bash.exe'))
    && (hasFile(path.win32.join(directory, 'msys-2.0.dll'))
      || hasFile(path.win32.join(directory, '..', 'usr', 'bin', 'msys-2.0.dll'))));
  if (!directory || /[\r\n]/.test(directory)) throw Error('Git Bash/MSYS bash not found. Install Git for Windows.');
  return path.win32.join(directory, 'bash.exe');
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try { console.log(bashInterpreter()); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}

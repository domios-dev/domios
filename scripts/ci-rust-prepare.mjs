// Caller: CI tests.yml
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const bundles = ['src-tauri/bundled-cli', 'src-tauri/bundled-embed'];
if (process.argv[2] !== 'cleanup' && process.platform === 'win32') {
  const powershell = path.join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0');
  if (!fs.existsSync(path.join(powershell, 'powershell.exe'))) throw new Error('Windows PowerShell missing');
  fs.appendFileSync(process.env.GITHUB_PATH, powershell + '\n');
}
if (process.argv[2] === 'cleanup') {
  for (const directory of bundles) {
    if (!fs.existsSync(directory)) continue;
    for (const file of fs.readdirSync(directory)) {
      const target = path.join(directory, file);
      if (fs.statSync(target).isFile() && fs.statSync(target).size === 0) fs.unlinkSync(target);
    }
  }
} else {
  fs.mkdirSync('dist', { recursive: true });
  fs.mkdirSync(bundles[0], { recursive: true });
  if (!fs.readdirSync(bundles[0]).length) fs.writeFileSync(path.join(bundles[0], process.platform === 'win32' ? 'codeterm.exe' : 'codeterm'), '');
  execFileSync(process.execPath, ['scripts/prepare-embed-stub.js'], { stdio: 'inherit' });
}

import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// An x64 host cross-compiles with the x64-hosted ARM64 tools; an arm64 host uses its native ones.
export function loadArm64MsvcEnvironment(vcvars, env = process.env, hostArch = process.arch) {
  const architecture = hostArch === 'arm64' ? 'arm64' : 'x64_arm64';
  const output = execFileSync(env.ComSpec, ['/d', '/s', '/c', `""${vcvars}" ${architecture} && set"`],
    { encoding: 'utf8', env, windowsVerbatimArguments: true });
  return Object.fromEntries(output.split(/\r?\n/).flatMap(line => {
    const match = /^([^=]+)=(.*)$/.exec(line);
    return match ? [[match[1], match[2]]] : [];
  }));
}

function main([target, sourceSha, tag, archive]) {
  const windows = target === 'aarch64-pc-windows-msvc';
  if (!windows && target !== 'aarch64-unknown-linux-gnu') throw new Error('Unsupported hosted target');
  if (!/^[a-f0-9]{40}$/.test(sourceSha ?? '')) throw new Error('Invalid source identity');
  if (process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Hosted compilation must not receive signing credentials');

  if (windows) {
    const vswhere = path.join(process.env['ProgramFiles(x86)'], 'Microsoft Visual Studio/Installer/vswhere.exe');
    const vcvars = execFileSync(vswhere, ['-latest', '-products', '*', '-find', 'VC/Auxiliary/Build/vcvarsall.bat'], { encoding: 'utf8' }).trim().split(/\r?\n/)[0];
    if (!vcvars) throw new Error('ARM64 Visual Studio toolchain missing');
    Object.assign(process.env, loadArm64MsvcEnvironment(vcvars));
  }

  const run = (command, args) => execFileSync(command, args, { stdio: 'inherit', timeout: 110 * 60_000 });
  writeFileSync('ci-rust-warnings.log', '');
  run('node', ['scripts/collect-rust-warnings.mjs', 'run', '--log', 'ci-rust-warnings.log', '--',
    ...(windows ? ['pwsh', '-NoProfile', '-File', 'build.ps1'] : ['bash', 'build.sh']), 'release-arm64-compile', tag, sourceSha, archive]);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2));

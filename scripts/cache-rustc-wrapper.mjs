// Caller: scripts/cache-policy.sh; scripts/tests/cache-rustc-wrapper.test.mjs
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function cargoBuildDirectory(manifest, { env = process.env, cwd = process.cwd() } = {}) {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--offline', '--no-deps',
    '--format-version=1', '--manifest-path', manifest], { env, cwd, encoding: 'utf8' }));
  if (typeof metadata.build_directory !== 'string' || !path.isAbsolute(metadata.build_directory)) {
    throw new Error('Cargo metadata did not report an absolute build directory');
  }
  return metadata.build_directory;
}

export function prepareCacheWrapper({ env = process.env } = {}) {
  const source = fileURLToPath(new URL('./cache-rustc-wrapper.rs', import.meta.url));
  const compilerName = process.platform === 'win32' ? 'rustc.exe' : 'rustc';
  const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { env, encoding: 'utf8' }).trim();
  const compiler = fs.realpathSync(path.join(sysroot, 'bin', compilerName));
  const clientName = process.platform === 'win32' ? 'sccache.exe' : 'sccache';
  const client = (env.PATH || env.Path || '').split(path.delimiter)
    .map(directory => path.resolve(directory, clientName))
    .find(file => fs.existsSync(file) && !fs.existsSync(file.replace(/(?:\.exe)?$/, '.compiler')));
  if (!client) throw new Error('The real sccache client was not found on PATH');
  const cacheClient = fs.realpathSync(client);
  const cargoHome = env.CARGO_HOME || path.join(os.homedir(), '.cargo');
  const outputPrefix = path.resolve(cargoHome, 'codeterm-cache-wrappers/compiler-output');
  const version = execFileSync(compiler, ['-vV'], { env, encoding: 'utf8' });
  const sourceText = fs.readFileSync(source, 'utf8').replaceAll('\r\n', '\n');
  const identity = createHash('sha256').update(sourceText)
    .update(compiler).update(version).update(outputPrefix).update(cacheClient).update(String(fs.statSync(compiler).mtimeMs)).digest('hex');
  const directory = path.join(cargoHome, 'codeterm-cache-wrappers', identity);
  const executable = path.join(directory, process.platform === 'win32' ? 'sccache.exe' : 'sccache');
  const manifest = path.join(directory, 'sccache.compiler');
  if (fs.existsSync(executable) && fs.existsSync(manifest) && fs.readFileSync(manifest, 'utf8') === compiler) return executable;
  fs.mkdirSync(directory, { recursive: true });
  const scratch = fs.mkdtempSync(path.join(directory, 'prepare-'));
  try {
    const pending = path.join(scratch, path.basename(executable));
    execFileSync(compiler, ['--edition=2021', '-Dwarnings', '-O', source, '-o', pending], {
      env: { ...env, CODETERM_CACHE_OUTPUT_PREFIX: outputPrefix, CODETERM_CACHE_CLIENT: cacheClient }, stdio: ['ignore', 'ignore', 'pipe'],
    });
    fs.writeFileSync(manifest, compiler);
    fs.renameSync(pending, executable);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
  return executable;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(process.argv[2] === '--build-directory'
    ? cargoBuildDirectory(process.argv[3] || 'src-tauri/Cargo.toml') : prepareCacheWrapper());
}

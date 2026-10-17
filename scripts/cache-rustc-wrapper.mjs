// Caller: scripts/cache-policy.sh; scripts/tests/cache-rustc-wrapper.test.mjs
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function prepareCacheWrapper({ env = process.env } = {}) {
  const source = fileURLToPath(new URL('./cache-rustc-wrapper.rs', import.meta.url));
  const compilerName = process.platform === 'win32' ? 'rustc.exe' : 'rustc';
  const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { env, encoding: 'utf8' }).trim();
  const compiler = fs.realpathSync(path.join(sysroot, 'bin', compilerName));
  const outputPrefix = path.resolve(path.dirname(source), '../.codeterm/compiler-output');
  const version = execFileSync(compiler, ['-vV'], { env, encoding: 'utf8' });
  const identity = createHash('sha256').update(source).update(fs.readFileSync(source))
    .update(compiler).update(version).update(outputPrefix).update(String(fs.statSync(compiler).mtimeMs)).digest('hex');
  const cargoHome = env.CARGO_HOME || path.join(os.homedir(), '.cargo');
  const directory = path.join(cargoHome, 'codeterm-cache-wrappers', identity);
  const executable = path.join(directory, process.platform === 'win32' ? 'sccache.exe' : 'sccache');
  const manifest = path.join(directory, 'sccache.compiler');
  if (fs.existsSync(executable) && fs.existsSync(manifest) && fs.readFileSync(manifest, 'utf8') === compiler) return executable;
  fs.mkdirSync(directory, { recursive: true });
  const scratch = fs.mkdtempSync(path.join(directory, 'prepare-'));
  try {
    const pending = path.join(scratch, path.basename(executable));
    execFileSync(compiler, ['--edition=2021', '-Dwarnings', '-O', source, '-o', pending], {
      env: { ...env, CODETERM_CACHE_OUTPUT_PREFIX: outputPrefix }, stdio: ['ignore', 'ignore', 'pipe'],
    });
    fs.writeFileSync(manifest, compiler);
    fs.renameSync(pending, executable);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
  return executable;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(prepareCacheWrapper());
}

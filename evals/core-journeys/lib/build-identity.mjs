import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

function required(match, label, file) {
  if (!match) throw new Error(`Could not resolve ${label} from ${file}`);
  return match[1];
}

function cargoSections(manifest) {
  const headings = [...manifest.matchAll(/^\[+(.+?)\]+[ \t]*$/gm)];
  return headings.map((heading, index) => ({
    name: heading[1],
    body: manifest.slice(heading.index + heading[0].length, headings[index + 1]?.index ?? manifest.length),
  }));
}

function cliBinaryName(root) {
  const manifestPath = path.join(root, 'cli/Cargo.toml');
  const manifest = readFileSync(manifestPath, 'utf8');
  const sections = cargoSections(manifest);
  const packageSection = sections.find((section) => section.name === 'package')?.body ?? '';
  const packageName = packageSection.match(/^name\s*=\s*"([^"]+)"/m);
  const defaultRun = packageSection.match(/^default-run\s*=\s*"([^"]+)"/m);
  const bins = sections.filter((section) => section.name === 'bin').map((section) => ({
    name: section.body.match(/^name\s*=\s*"([^"]+)"/m),
    source: section.body.match(/^path\s*=\s*"([^"]+)"/m),
  }));
  const main = bins.find((bin) => bin.source?.[1] === 'src/main.rs') ?? bins.find((bin) => bin.name?.[1] === defaultRun?.[1]);
  return required(main?.name ?? defaultRun ?? packageName, 'CLI binary name', manifestPath);
}

function shellInput(source, key, file) {
  const match = source.match(new RegExp(`\\bCT_${key}\\s*=\\s*"\\$\\{([A-Z][A-Z0-9_]*):-`));
  return required(match, `CT_${key} input environment`, file);
}

function exportedDevInput(source, key, file) {
  return required(source.match(new RegExp(`\\bexport\\s+([A-Z][A-Z0-9_]*)="\\$CT_${key}"`)), `development ${key.toLowerCase()} environment`, file);
}

function cliInput(source, suffix, file) {
  return required(source.match(new RegExp(`std::env::var(?:_os)?\\("([A-Z][A-Z0-9_]*${suffix})"\\)`)), `CLI ${suffix.toLowerCase()} environment`, file);
}

export function readBuildIdentity(checkout, { env = process.env } = {}) {
  const root = path.resolve(checkout);
  const buildFile = path.join(root, 'build.sh');
  const build = readFileSync(buildFile, 'utf8');
  const cliIdentify = path.join(root, 'cli/src/cmd/identify.rs');
  const identify = readFileSync(cliIdentify, 'utf8');
  const binaryName = cliBinaryName(root);
  const instanceInputEnv = shellInput(build, 'INSTANCE', buildFile);
  const portInputEnv = shellInput(build, 'PORT', buildFile);
  const devInstanceEnv = exportedDevInput(build, 'INSTANCE', buildFile);
  const devPortEnv = exportedDevInput(build, 'PORT', buildFile);
  const cliInstanceEnv = cliInput(identify, 'INSTANCE', cliIdentify);
  const cliPortEnv = cliInput(identify, 'API_PORT', cliIdentify);
  const instance = env.CORE_JOURNEY_REQUESTED_INSTANCE ?? env[instanceInputEnv] ?? env[cliInstanceEnv] ?? '';
  const targetRoot = instance ? path.join(root, 'src-tauri/target/named-dev') : env.CARGO_TARGET_DIR ?? path.join(root, 'src-tauri/target');
  const targetProfile = env.CARGO_BUILD_TARGET ? path.join(targetRoot, env.CARGO_BUILD_TARGET) : targetRoot;
  const executable = path.join(targetProfile, 'debug', `${binaryName}${process.platform === 'win32' ? '.exe' : ''}`);
  return {
    root, binaryName, executable,
    request: { instance: instanceInputEnv, port: portInputEnv },
    development: { instance: devInstanceEnv, port: devPortEnv },
    cli: {
      instance: cliInstanceEnv,
      port: cliPortEnv,
      apiToken: cliInput(identify, '_API_TOKEN', cliIdentify),
      dataDir: cliInput(identify, '_DATA_DIR', cliIdentify),
      socketPath: cliInput(identify, '_SOCKET_PATH', cliIdentify),
    },
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if ((args.length !== 2 && args.length !== 4) || args[0] !== '--checkout' || (args.length === 4 && args[2] !== '--field')) {
      throw new Error('Usage: build-identity.mjs --checkout PATH [--field FIELD]');
    }
    const identity = readBuildIdentity(args[1]);
    if (args.length === 2) process.stdout.write(`${JSON.stringify(identity)}\n`);
    else {
      const value = args[3].split('.').reduce((current, key) => current?.[key], identity);
      if (typeof value !== 'string' || !value) throw new Error(`Unknown or empty identity field: ${args[3]}`);
      process.stdout.write(`${value}\n`);
    }
  } catch (error) {
    console.error(`build identity: ${error.message}`);
    process.exitCode = 2;
  }
}

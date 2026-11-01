#!/usr/bin/env bash

build_dev_update_installer() {
  if [[ "$OSTYPE" != msys* && "$OSTYPE" != cygwin* ]]; then
    echo "dev-update-installer requires Windows" >&2
    return 2
  fi
  if [ "$#" -ne 2 ] || [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    echo "Usage: ./build.sh dev-update-installer <fixture-public-key-file> <launch-context-file>; fixture signing key required in TAURI_SIGNING_PRIVATE_KEY" >&2
    return 2
  fi
  local public_key="$1" launch="$2" logs conf bundle_conf
  local target_args=()
  [ -z "${CARGO_BUILD_TARGET:-}" ] || target_args=(--target "$CARGO_BUILD_TARGET")
  unset UPDATER_TOKEN
  configure_dev_cargo
  ensure_deps
  logs="$(tauri_debug_dir)/.domios-update-build"
  mkdir -p "$logs"
  conf="$logs/compile.json"
  bundle_conf="$logs/bundle.json"
  node --input-type=module - "$public_key" "$launch" "$conf" <<'NODE'
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
const [keyFile, launchFile, output] = process.argv.slice(2);
for (const file of [keyFile, launchFile]) {
  if (!fs.lstatSync(file).isFile() || fs.lstatSync(file).isSymbolicLink()) throw new Error('Fixture inputs must be regular files');
}
const launchBytes = fs.readFileSync(launchFile);
if (launchBytes.length > 8192) throw new Error('Launch context exceeds limit');
const launch = JSON.parse(launchBytes.toString('utf8'));
const fields = ['install_root', 'instance', 'data_dir', 'config_dir', 'port', 'fixture_root'];
if (Object.keys(launch).some(key => ![...fields, 'diagnostics_port'].includes(key)) || fields.some(key => !(key in launch))) throw new Error('Unexpected launch context fields');
if (launch.instance !== process.env.CODETERM_DEV_INSTANCE || String(launch.port) !== process.env.CODETERM_DEV_API_PORT) throw new Error('Launch context differs from the named build instance');
if (!/^[a-z0-9-]{1,64}$/.test(launch.instance) || !Number.isInteger(launch.port) || launch.port < 1024 || launch.port > 65535 || [7685, 7686].includes(launch.port)) throw new Error('Invalid isolated DEV instance or port');
if ('diagnostics_port' in launch && (!Number.isInteger(launch.diagnostics_port) || launch.diagnostics_port < 1024 || launch.diagnostics_port > 65535 || [7685, 7686, launch.port].includes(launch.diagnostics_port))) throw new Error('Invalid isolated diagnostics port');
for (const key of ['install_root', 'data_dir', 'config_dir', 'fixture_root']) {
  if (!path.isAbsolute(launch[key]) || !fs.lstatSync(launch[key]).isDirectory() || fs.lstatSync(launch[key]).isSymbolicLink()) throw new Error('Launch directories must be existing absolute directories');
}
if (fs.realpathSync(launchFile) !== path.join(fs.realpathSync(launch.install_root), '.domios-isolated-update.json') ||
    fs.realpathSync(launch.fixture_root) !== path.join(fs.realpathSync(launch.data_dir), 'update-fixtures')) throw new Error('Launch context escaped its fixed install/data root');
const installRoot = fs.realpathSync(launch.install_root).toLowerCase();
const privateDirectories = [launch.data_dir, launch.config_dir].map(directory => fs.realpathSync(directory).toLowerCase());
const home = fs.realpathSync(os.homedir()).toLowerCase();
if (privateDirectories[0] === privateDirectories[1] || privateDirectories.some(directory => directory === installRoot || directory.startsWith(installRoot + path.sep) ||
    directory.startsWith(home + path.sep) && /^(\.codeterm|\.domios)/.test(path.relative(home, directory)))) throw new Error('Private directories overlap installation or owner data');
const pubkey = fs.readFileSync(keyFile, 'utf8').trim();
const base = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
if (!pubkey || pubkey === base.plugins.updater.pubkey) throw new Error('A separate fixture public key is required');
const contextCopy = path.join(path.dirname(output), '.domios-isolated-update.json');
if (fs.existsSync(contextCopy)) {
  if (!fs.lstatSync(contextCopy).isFile() || fs.lstatSync(contextCopy).isSymbolicLink() || !fs.readFileSync(contextCopy).equals(launchBytes)) throw new Error('Stale or unproven staged launch context');
} else {
  fs.writeFileSync(contextCopy, launchBytes, { flag: 'wx' });
}
if (!fs.readFileSync(contextCopy).equals(launchBytes)) throw new Error('Staged launch context differs from validated input');
fs.writeFileSync(path.join(path.dirname(output), 'launch-context-source.json'), JSON.stringify({
  source: fs.realpathSync.native(launchFile), sha256: createHash('sha256').update(launchBytes).digest('hex'),
}));
const config = {
  identifier: 'com.domios.dev.updater', productName: 'Domios Update Proof', mainBinaryName: 'domios-dev-app',
  build: { devUrl: null, beforeDevCommand: null, frontendDist: path.resolve('dist') },
  bundle: { resources: [], externalBin: [], createUpdaterArtifacts: true,
    windows: { nsis: { installMode: 'currentUser', installerHooks: path.resolve('src-tauri/windows/installer-hooks.nsh') } } },
  plugins: { updater: { pubkey, requireSignedVersion: true }, 'deep-link': { desktop: { schemes: [] } } },
};
fs.writeFileSync(output, JSON.stringify(config));
NODE
  build_dev_frontend
  build_dev_cli "$logs/cli.jsonl"
  export CODETERM_SERVER_CLI_SHA256="$built_cli_hash"
  ct_with_firebase_build_config build_dev_server "$logs/server.jsonl"
  cargo tree --manifest-path src-tauri/Cargo.toml -p domios-server --no-default-features \
    --features domios-server/embeddings,domios-server/mem-postgres --edges normal,build --prefix none > "$logs/server-tree.txt"
  build_dev_client "$conf" --manifest-path src-tauri/Cargo.toml -p codeterm --bin domios-app \
    --profile dev --no-default-features --features tauri/custom-protocol,dev-updater-allowlocal
  node scripts/server-artifact.mjs stage-dev "$logs/server.jsonl" "$logs/server-tree.txt" \
    "$logs/cli.jsonl" "$(pwd -P)" "$(dev_resource_dir)" "$(dev_built_app_path)" "$CODETERM_BUILD_SHA"
  node --input-type=module - "$conf" "$bundle_conf" "$(dev_resource_dir)" "$launch" <<'NODE'
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
const [compile, output, staged, launch] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(compile, 'utf8'));
const project = fs.realpathSync.native(path.resolve('src-tauri'));
const contextCopy = path.join(path.dirname(compile), '.domios-isolated-update.json');
const contextSource = JSON.parse(fs.readFileSync(path.join(path.dirname(compile), 'launch-context-source.json'), 'utf8'));
for (const file of [launch, contextCopy]) {
  if (!fs.lstatSync(file).isFile() || fs.lstatSync(file).isSymbolicLink()) throw new Error('Launch context source must remain a regular file');
  if (createHash('sha256').update(fs.readFileSync(file)).digest('hex') !== contextSource.sha256) throw new Error('Launch context changed after validation');
}
if (fs.realpathSync.native(launch) !== contextSource.source || !fs.readFileSync(contextCopy).equals(fs.readFileSync(launch))) throw new Error('Launch context provenance changed');
const resourceKey = source => {
  const relative = path.relative(project, fs.realpathSync.native(source));
  if (path.isAbsolute(relative) || relative === '..' || relative.startsWith('..' + path.sep)) throw new Error('DEV resources must stay inside the canonical project');
  return relative.split(path.sep).join('/');
};
const resources = {};
for (const name of ['server-component.json', 'domios-server', 'cli', 'dist', 'skills', 'LICENSE', 'NOTICE', 'conpty']) {
  const source = path.resolve(staged, name);
  const stat = fs.lstatSync(source);
  if (stat.isSymbolicLink()) throw new Error('Staged resource must not be a link');
  resources[resourceKey(source)] = name;
}
resources[resourceKey(contextCopy)] = '.domios-isolated-update.json';
config.bundle.resources = resources;
fs.writeFileSync(output, JSON.stringify(config));
NODE
  pnpm tauri bundle --debug --no-binary-patching "${target_args[@]}" --config "$bundle_conf" --bundles nsis
  node --input-type=module - "$logs" "$(dev_resource_dir)" "$CODETERM_BUILD_SHA" "$launch" <<'NODE'
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
const [logs, staged, source, launch] = process.argv.slice(2);
const version = JSON.parse(fs.readFileSync('package.json', 'utf8')).version;
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const contextCopy = path.join(logs, '.domios-isolated-update.json');
const contextSource = JSON.parse(fs.readFileSync(path.join(logs, 'launch-context-source.json'), 'utf8'));
for (const file of [launch, contextCopy]) {
  if (!fs.lstatSync(file).isFile() || fs.lstatSync(file).isSymbolicLink() || hash(file) !== contextSource.sha256) throw new Error('Packaged launch context provenance changed');
}
if (fs.realpathSync.native(launch) !== contextSource.source) throw new Error('Launch context source identity changed');
const manifest = JSON.parse(fs.readFileSync(path.join(staged, 'server-component.json'), 'utf8'));
const client = path.join(staged, 'domios-dev-app.exe');
if (manifest.server_build !== source || manifest.client_identity.sha256 !== hash(client)) throw new Error('Packaged DEV client identity changed');
if (manifest.server_sha256 !== hash(path.join(staged, 'domios-server/domios-server.exe'))) throw new Error('Packaged DEV server identity changed');
const bundleRoot = path.join(staged, 'bundle/nsis');
const installers = fs.readdirSync(bundleRoot).filter(name => name.includes(`_${version}_`) && name.endsWith('-setup.exe'));
if (installers.length !== 1) throw new Error('Expected one installer for the exact source version');
const installer = path.join(bundleRoot, installers[0]);
if (!fs.statSync(installer + '.sig').isFile()) throw new Error('Signed updater artifact is missing');
const receipt = { source, version, installer: path.resolve(installer), installer_sha256: hash(installer),
  client_sha256: hash(client), server_sha256: manifest.server_sha256,
  cli_sha256: hash(path.join(staged, 'cli/codeterm.exe')),
  launch_context: { source: contextSource.source, staged: path.resolve(contextCopy), sha256: contextSource.sha256 } };
const file = path.join(logs, 'receipt.json');
fs.writeFileSync(file, JSON.stringify(receipt, null, 2));
console.log(`DEV updater installer receipt: ${path.resolve(file)}`);
NODE
}

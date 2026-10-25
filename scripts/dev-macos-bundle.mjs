import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

export function macDevSigning(env = process.env) {
  const { APPLE_SIGNING_IDENTITY: identity, APPLE_TEAM_ID: team, RELEASE_KEYCHAIN: keychain } = env;
  if (!identity?.startsWith('Developer ID Application: ') || !/^[A-Z0-9]{10}$/.test(team ?? '') || !keychain) {
    throw new Error('Named macOS DEV requires a Developer ID identity, team and keychain');
  }
  return { identity, team, keychain, clientIdentity: {
    kind: 'macos_designated_requirement', identifier: 'com.domios.app', team_id: team,
    requirement: `anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "${team}"`,
  } };
}

const command = (program, args) => execFileSync(program, args, { encoding: 'utf8', timeout: 120000, stdio: ['ignore', 'pipe', 'pipe'] });

export function signMacDevServer(file, signing, run = command) {
  run('codesign', ['--force', '--options', 'runtime', '--timestamp=none', '--keychain', signing.keychain,
    '--sign', signing.identity, file]);
  run('codesign', ['--verify', '--strict', '--test-requirement',
    `=anchor apple generic and certificate leaf[subject.OU] = "${signing.team}"`, file]);
}

export function sealMacDevBundle({ root, repo, signing, run = command }) {
  const contents = path.dirname(root);
  const app = path.dirname(contents);
  if (path.basename(app) !== 'Domios.app' || path.basename(contents) !== 'Contents' || path.basename(root) !== 'Resources') {
    throw new Error('Named macOS DEV resources must be inside Domios.app/Contents/Resources');
  }
  writeFileSync(path.join(contents, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.domios.app</string>
<key>CFBundleExecutable</key><string>domios-dev-app</string>
<key>CFBundleName</key><string>Domios Dev</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>Domios uses the microphone for voice input.</string>
</dict></plist>\n`);
  const frozen = ['../MacOS/domios-server', 'cli/codeterm', 'server-component.json']
    .map(relative => [path.resolve(root, relative), readFileSync(path.resolve(root, relative))]);
  run('codesign', ['--force', '--options', 'runtime', '--timestamp=none', '--keychain', signing.keychain,
    '--entitlements', path.join(repo, 'src-tauri/Entitlements.plist'), '--sign', signing.identity, app]);
  for (const [file, bytes] of frozen) {
    if (!readFileSync(file).equals(bytes)) throw new Error('macOS DEV signing changed frozen pair bytes: ' + file);
  }
  run('codesign', ['--verify', '--strict', '--deep', '--test-requirement', '=' + signing.clientIdentity.requirement, app]);
}

import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const nativeIdentity = [
  'cli/src/cmd/install.rs', 'cli/src/cmd/system.rs', 'src-tauri/build.rs', 'src-tauri/Info.plist', 'src-tauri/src/deep_links.rs', 'src-tauri/src/deep_links/macos.rs',
  'src-tauri/src/startup/desktop_lifecycle/model.rs',
  'src-tauri/crates/codeterm-process-identity/src/client_identity.rs',
  'src-tauri/crates/codeterm-types/src/native_update.rs',
  'src-tauri/crates/codeterm-client-shell/src/autostart.rs',
  'src-tauri/crates/codeterm-api/src/health_routes.rs',
  'src-tauri/crates/codeterm-browser/src/profile.rs', 'src-tauri/crates/codeterm-browser/src/process.rs',
  'src-tauri/crates/codeterm-plugins/src/util.rs', 'src-tauri/crates/codeterm-plugins/src/install.rs',
  'src-tauri/crates/codeterm-plugin/src/model_catalog/readme.rs',
  'scripts/package-server-component-manifest.mjs', 'scripts/verify-updater-manifest.mjs',
];

export function identityViolations(file, text) {
  const violations = [];
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    if (/\bCodeTerm\b|com\.codeterm\.(?:app|dev)\b/.test(line)) violations.push(`${file}:${index + 1}: ${line.trim()}`);
  }
  return violations;
}

export function checkInstalledIdentity(repo = root) {
  const files = [
    ...nativeIdentity,
    ...readdirSync(path.join(repo, '.agents/skills/github-release/scripts')).filter(name => /\.(?:sh|ps1)$/.test(name)).map(name => `.agents/skills/github-release/scripts/${name}`),
    ...readdirSync(path.join(repo, 'src-tauri')).filter(name => /^tauri.*\.json$/.test(name)).map(name => `src-tauri/${name}`),
    ...readdirSync(path.join(repo, '.github/workflows')).filter(name => /^release.*\.yml$/.test(name)).map(name => `.github/workflows/${name}`),
    ...readdirSync(path.join(repo, 'scripts')).filter(name => /^release-.*\.(?:mjs|sh|ps1)$/.test(name)).map(name => `scripts/${name}`),
  ];
  return files.flatMap(file => identityViolations(file, readFileSync(path.join(repo, file), 'utf8')));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const violations = checkInstalledIdentity();
  for (const violation of violations) console.error(violation);
  console.log(`installed identity: ${violations.length} violations`);
  process.exitCode = violations.length ? 1 : 0;
}

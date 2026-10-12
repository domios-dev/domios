import path from 'node:path';

export function ownedSidecar(command, dataDir, platform = process.platform) {
  const paths = platform === 'win32' ? path.win32 : path.posix;
  const root = paths.resolve(dataDir);
  if (!/^\.codeterm-dev(?:-[a-zA-Z0-9_-]+)?$/.test(paths.basename(root))) return false;
  if (!/(?:^|[\s/\\])codex(?:\.js|\.exe)?["']?\s+app-server(?:\s|$)/.test(command)) return false;
  const normalize = value => platform === 'win32' ? paths.normalize(value).replaceAll('\\', '/').toLowerCase() : paths.normalize(value);
  const tokenRoot = normalize(paths.join(root, 'codex-rpc')) + '/';
  const escaped = tokenRoot.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const normalizedCommand = platform === 'win32' ? command.replaceAll('\\', '/').toLowerCase() : command;
  return new RegExp(`(?:^|\\s)--ws-token-file\\s+["']?${escaped}[^\\s"'/\\\\]+\\.token["']?(?=\\s|$)`).test(normalizedCommand);
}

export function ownedGroups(processes, dataDir, platform = process.platform) {
  return processes.filter(process => process.pid > 1
    && (platform === 'win32' || process.pid === process.group)
    && ownedSidecar(process.command, dataDir, platform));
}

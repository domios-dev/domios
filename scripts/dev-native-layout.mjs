import path from 'node:path';

export function stagedDevPaths(client, { platform = process.platform, instance = process.env.CODETERM_DEV_INSTANCE } = {}) {
  const directory = path.dirname(client);
  const mac = platform === 'darwin' && Boolean(instance);
  const resources = platform === 'win32' ? directory : path.join(directory,
    mac ? '.domios-dev-runtime/Domios.app/Contents/Resources' : '.domios-dev-runtime/lib/codeterm');
  return {
    mac, resources,
    client: platform === 'win32' ? path.join(directory, 'domios-dev-app.exe')
      : path.resolve(resources, mac ? '../MacOS/domios-dev-app' : '../../bin/domios-dev-app'),
    serverRelative: mac ? '../MacOS/domios-server' : `domios-server/domios-server${platform === 'win32' ? '.exe' : ''}`,
  };
}

export function devResourceRoot(app, platform = process.platform) {
  const directory = path.dirname(app);
  if (platform === 'win32') return directory;
  return path.resolve(directory, platform === 'darwin' && path.basename(path.dirname(directory)) === 'Contents'
    && path.basename(path.dirname(path.dirname(directory))) === 'Domios.app' ? '../Resources' : '../lib/codeterm');
}

export function devServerPath(app, platform = process.platform) {
  const root = devResourceRoot(app, platform);
  return platform === 'darwin' && path.basename(root) === 'Resources'
    ? path.resolve(root, '../MacOS/domios-server')
    : path.join(root, 'domios-server', `domios-server${platform === 'win32' ? '.exe' : ''}`);
}
